use crate::{config::AuthMode, db, AppState};
use axum::{
    extract::{ConnectInfo, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Redirect},
    routing::{get, post},
    Json, Router,
};
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{jwk, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/auth/login", post(login).get(login))
        .route("/auth/callback", get(oidc_callback))
        .route("/auth/logout", post(logout))
}

#[derive(Deserialize)]
struct LoginForm {
    password: String,
}
#[derive(Serialize)]
struct SessionOut {
    username: &'static str,
    csrf_token: String,
    mode: &'static str,
}

async fn login(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Option<Json<LoginForm>>,
) -> Result<impl IntoResponse, StatusCode> {
    match state.config.auth.mode {
        AuthMode::Local => {
            let Json(form) = body.ok_or(StatusCode::BAD_REQUEST)?;
            if !allow_login(&state, addr.ip()) {
                return Err(StatusCode::TOO_MANY_REQUESTS);
            }
            let local = state.config.auth.local.as_ref().unwrap();
            let expected = std::fs::read_to_string(&local.password_file)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            if !constant_eq(form.password.as_bytes(), expected.trim().as_bytes()) {
                return Err(StatusCode::UNAUTHORIZED);
            }
            let (token, csrf) =
                db::create_session(&state.pool, state.config.auth.session_idle_hours)
                    .await
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            Ok((
                session_headers(&state, &token),
                Json(SessionOut {
                    username: "owner",
                    csrf_token: csrf,
                    mode: state.auth_mode(),
                }),
            )
                .into_response())
        }
        AuthMode::Oidc => {
            let oidc = state.config.auth.oidc.as_ref().unwrap();
            let metadata = oidc_metadata(&state)
                .await
                .map_err(|_| StatusCode::BAD_GATEWAY)?;
            let csrf = db::random_token(16);
            let nonce = db::random_token(16);
            let verifier = db::random_token(48);
            let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(Sha256::digest(verifier.as_bytes()));
            sqlx::query(
                "INSERT INTO oidc_transactions(state,nonce,verifier,expires_at) VALUES (?,?,?,?)",
            )
            .bind(&csrf)
            .bind(&nonce)
            .bind(&verifier)
            .bind(Utc::now() + Duration::minutes(10))
            .execute(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let redirect = format!(
                "{}/auth/callback",
                state.config.public_url.trim_end_matches('/')
            );
            let url=format!("{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&nonce={}&code_challenge={}&code_challenge_method=S256",metadata.authorization_endpoint,urlencoding(&oidc.client_id),urlencoding(&redirect),urlencoding("openid profile email"),urlencoding(&csrf),urlencoding(&nonce),urlencoding(&challenge));
            Ok(Redirect::temporary(&url).into_response())
        }
        AuthMode::Proxy => {
            proxy_identity(&state, &headers, addr).ok_or(StatusCode::UNAUTHORIZED)?;
            let (token, csrf) =
                db::create_session(&state.pool, state.config.auth.session_idle_hours)
                    .await
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            Ok((
                session_headers(&state, &token),
                Json(SessionOut {
                    username: "owner",
                    csrf_token: csrf,
                    mode: state.auth_mode(),
                }),
            )
                .into_response())
        }
    }
}

#[derive(Deserialize)]
struct Callback {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}
async fn oidc_callback(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(q): axum::extract::Query<Callback>,
) -> Result<impl IntoResponse, StatusCode> {
    if state.config.auth.mode != AuthMode::Oidc {
        return Err(StatusCode::NOT_FOUND);
    }
    if q.error.is_some() {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let code = q.code.ok_or(StatusCode::BAD_REQUEST)?;
    let sid = q.state.ok_or(StatusCode::BAD_REQUEST)?;
    let row = sqlx::query(
        "DELETE FROM oidc_transactions WHERE state=? RETURNING nonce,verifier,expires_at",
    )
    .bind(&sid)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::UNAUTHORIZED)?;
    let expires: DateTime<Utc> = row.get("expires_at");
    if expires < Utc::now() {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let nonce: String = row.get("nonce");
    let verifier: String = row.get("verifier");
    let oidc = state.config.auth.oidc.as_ref().unwrap();
    let metadata = oidc_metadata(&state)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    let secret = match &oidc.client_secret_file {
        Some(p) => Some(
            std::fs::read_to_string(p)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .trim()
                .to_string(),
        ),
        None => None,
    };
    let redirect = format!(
        "{}/auth/callback",
        state.config.public_url.trim_end_matches('/')
    );
    let mut form = HashMap::from([
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", redirect.as_str()),
        ("client_id", oidc.client_id.as_str()),
        ("code_verifier", verifier.as_str()),
    ]);
    if let Some(s) = secret.as_deref() {
        form.insert("client_secret", s);
    }
    #[derive(Deserialize)]
    struct TokenResponse {
        id_token: String,
    }
    let token = state
        .http
        .post(&metadata.token_endpoint)
        .form(&form)
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?
        .error_for_status()
        .map_err(|_| StatusCode::UNAUTHORIZED)?
        .json::<TokenResponse>()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    validate_id_token(&state, &metadata, &token.id_token, &nonce).await?;
    let (session, csrf) = db::create_session(&state.pool, state.config.auth.session_idle_hours)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut response = Redirect::to("/").into_response();
    let h = session_headers(&state, &session);
    response.headers_mut().extend(h);
    response
        .headers_mut()
        .insert("x-csrf-token", csrf.parse().unwrap());
    Ok(response)
}

async fn logout(State(state): State<Arc<AppState>>, headers: HeaderMap) -> impl IntoResponse {
    if let Some(token) = session_cookie(&headers) {
        let _ = db::logout(&state.pool, &token).await;
    }
    (
        [(header::SET_COOKIE, clear_cookie())],
        StatusCode::NO_CONTENT,
    )
}

pub async fn authenticated(
    state: &AppState,
    headers: &HeaderMap,
    addr: SocketAddr,
) -> Result<Option<String>, StatusCode> {
    match state.config.auth.mode {
        AuthMode::Proxy => {
            if proxy_identity(state, headers, addr).is_some() {
                Ok(Some(db::random_token(8)))
            } else {
                Err(StatusCode::UNAUTHORIZED)
            }
        }
        _ => {
            let token = session_cookie(headers).ok_or(StatusCode::UNAUTHORIZED)?;
            db::validate_session(&state.pool, &token, state.config.auth.session_idle_hours)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
pub fn session_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|p| p.trim().strip_prefix("myrss_session=").map(str::to_owned))
}
pub fn session_headers(state: &AppState, token: &str) -> HeaderMap {
    let mut h = HeaderMap::new();
    let secure = state.config.public_url.starts_with("https://");
    h.insert(
        header::SET_COOKIE,
        format!(
            "myrss_session={}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{}",
            token,
            state.config.auth.session_idle_hours * 3600,
            if secure { "; Secure" } else { "" }
        )
        .parse()
        .unwrap(),
    );
    h
}
fn clear_cookie() -> &'static str {
    "myrss_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0"
}
fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    let ah = Sha256::digest(a);
    let bh = Sha256::digest(b);
    ah == bh
}
fn urlencoding(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}

#[derive(Deserialize)]
struct Metadata {
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
}
async fn oidc_metadata(state: &AppState) -> Result<Metadata, StatusCode> {
    let oidc = state.config.auth.oidc.as_ref().unwrap();
    state
        .http
        .get(format!(
            "{}/.well-known/openid-configuration",
            oidc.issuer_url.trim_end_matches('/')
        ))
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?
        .error_for_status()
        .map_err(|_| StatusCode::BAD_GATEWAY)?
        .json()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)
}
#[derive(Deserialize)]
struct Claims {
    sub: String,
    nonce: Option<String>,
}
async fn validate_id_token(
    state: &AppState,
    metadata: &Metadata,
    token: &str,
    nonce: &str,
) -> Result<(), StatusCode> {
    let header = jsonwebtoken::decode_header(token).map_err(|_| StatusCode::UNAUTHORIZED)?;
    let jwks: serde_json::Value = state
        .http
        .get(&metadata.jwks_uri)
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?
        .json()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    let key = jwks["keys"]
        .as_array()
        .and_then(|keys| {
            keys.iter()
                .find(|k| k["kid"].as_str() == header.kid.as_deref())
        })
        .cloned()
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let jwk: jwk::Jwk = serde_json::from_value(key).map_err(|_| StatusCode::UNAUTHORIZED)?;
    if !matches!(
        header.alg,
        Algorithm::RS256
            | Algorithm::RS384
            | Algorithm::RS512
            | Algorithm::ES256
            | Algorithm::ES384
    ) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let mut validation = Validation::new(header.alg);
    validation.set_audience(&[state.config.auth.oidc.as_ref().unwrap().client_id.as_str()]);
    validation.set_issuer(&[state
        .config
        .auth
        .oidc
        .as_ref()
        .unwrap()
        .issuer_url
        .trim_end_matches('/')]);
    let data = jsonwebtoken::decode::<Claims>(
        token,
        &DecodingKey::from_jwk(&jwk).map_err(|_| StatusCode::UNAUTHORIZED)?,
        &validation,
    )
    .map_err(|_| StatusCode::UNAUTHORIZED)?;
    if data.claims.nonce.as_deref() != Some(nonce) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if let Some(expected) = &state.config.auth.oidc.as_ref().unwrap().subject {
        if expected != &data.claims.sub {
            return Err(StatusCode::FORBIDDEN);
        }
    }
    Ok(())
}
fn allow_login(state: &AppState, ip: std::net::IpAddr) -> bool {
    let mut attempts = state.login_attempts.lock().unwrap();
    let now = std::time::Instant::now();
    attempts.retain(|_, (_, at)| now.duration_since(*at) < std::time::Duration::from_secs(60));
    let entry = attempts.entry(ip).or_insert((0, now));
    entry.0 += 1;
    entry.1 = now;
    entry.0 <= 10
}

fn proxy_identity(state: &AppState, headers: &HeaderMap, addr: SocketAddr) -> Option<()> {
    let proxy = state.config.auth.proxy.as_ref()?;
    let ip = addr.ip();
    let trusted = proxy.trusted_peer_cidrs.iter().any(|c| {
        c.parse::<ipnet::IpNet>()
            .is_ok_and(|network| network.contains(&ip))
    });
    if !trusted {
        return None;
    }
    let v = headers.get(&proxy.user_header)?.to_str().ok()?;
    if v.is_empty() {
        None
    } else {
        Some(())
    }
}
