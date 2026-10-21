use super::auth::{self};
use crate::{
    db::{self, EntryQuery},
    ingest::opml,
    models::{AppCredentialCreated, AppCredentialView, EntryState},
    security::validate_url,
    AppState,
};
use argon2::{password_hash::SaltString, Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::{
    extract::{ConnectInfo, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use md5::Md5;
use serde::Deserialize;
use sha2::Digest;
use sqlx::Row;
use std::{net::SocketAddr, sync::Arc};

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/session", get(session))
        .route("/auth-mode", get(auth_mode))
        .route("/snapshot", get(snapshot))
        .route("/entries", get(entries))
        .route("/entries/{id}", get(entry).post(set_state))
        .route("/entries/read-all", post(mark_all_read))
        .route("/feeds", post(add_feed))
        .route("/feeds/{id}", delete(delete_feed).post(update_feed))
        .route("/feeds/{id}/refresh", post(refresh_feed))
        .route("/opml", get(export_opml).post(import_opml))
        .route(
            "/app-credentials",
            get(list_credentials).post(create_credential),
        )
        .route("/app-credentials/{id}", delete(revoke_credential))
        .route("/backup", post(backup))
}

type ApiResult<T> = Result<T, StatusCode>;
async fn auth(state: &AppState, h: &HeaderMap, a: SocketAddr) -> ApiResult<Option<String>> {
    auth::authenticated(state, h, a).await
}
fn csrf(state: &AppState, h: &HeaderMap, session: Option<String>) -> ApiResult<()> {
    match state.config.auth.mode {
        crate::config::AuthMode::Proxy => {
            if h.get("x-myrss-csrf").and_then(|v| v.to_str().ok()) == Some("proxy") {
                Ok(())
            } else {
                Err(StatusCode::FORBIDDEN)
            }
        }
        _ => {
            if h.get("x-csrf-token").and_then(|v| v.to_str().ok()) == session.as_deref() {
                Ok(())
            } else {
                Err(StatusCode::FORBIDDEN)
            }
        }
    }
}

async fn auth_mode(State(s): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({"mode":s.auth_mode()}))
}
async fn session(
    State(s): State<Arc<AppState>>,
    ConnectInfo(a): ConnectInfo<SocketAddr>,
    h: HeaderMap,
) -> ApiResult<impl IntoResponse> {
    let csrf = auth(&s, &h, a).await?;
    Ok(Json(
        serde_json::json!({"user":"owner","csrf_token":csrf,"mode":s.auth_mode()}),
    ))
}
async fn snapshot(
    State(s): State<Arc<AppState>>,
    ConnectInfo(a): ConnectInfo<SocketAddr>,
    h: HeaderMap,
) -> ApiResult<Json<crate::models::Snapshot>> {
    auth(&s, &h, a).await?;
    db::snapshot(&s.pool, s.auth_mode(), s.config.server.remote_media)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
async fn entries(
    State(s): State<Arc<AppState>>,
    ConnectInfo(a): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Query(q): Query<EntryQuery>,
) -> ApiResult<impl IntoResponse> {
    auth(&s, &h, a).await?;
    let entries = db::list_entries(&s.pool, &q)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let full_page = entries.len() as i64 == q.limit.unwrap_or(50).clamp(1, 200);
    let next = if full_page {
        entries.last().map(|e| e.id)
    } else {
        None
    };
    Ok(Json(
        serde_json::json!({"entries":entries,"next_before":next}),
    ))
}
async fn entry(
    State(s): State<Arc<AppState>>,
    ConnectInfo(a): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<impl IntoResponse> {
    auth(&s, &h, a).await?;
    db::get_entry(&s.pool, id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}
async fn set_state(
    State(s): State<Arc<AppState>>,
    ConnectInfo(a): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<EntryState>,
) -> ApiResult<StatusCode> {
    let session = auth(&s, &h, a).await?;
    csrf(&s, &h, session)?;
    db::set_entry_state(&s.pool, id, &body)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn mark_all_read(
    State(s): State<Arc<AppState>>,
    ConnectInfo(a): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> ApiResult<Json<serde_json::Value>> {
    let session = auth(&s, &h, a).await?;
    csrf(&s, &h, session)?;
    let feed_id = body.get("feed_id").and_then(|v| v.as_i64());
    let n = db::mark_all_read(&s.pool, feed_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({"updated":n})))
}

#[derive(Deserialize)]
struct FeedIn {
    url: String,
    title: Option<String>,
    folder: Option<String>,
}
async fn add_feed(
    State(s): State<Arc<AppState>>,
    ConnectInfo(a): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Json(body): Json<FeedIn>,
) -> ApiResult<impl IntoResponse> {
    let session = auth(&s, &h, a).await?;
    csrf(&s, &h, session)?;
    let requested_title = body.title.clone();
    let id = match s
        .fetcher
        .discover_and_add(&s.pool, &body.url, body.folder.as_deref())
        .await
    {
        Ok(id) => id,
        Err(_e) => return Err(StatusCode::BAD_GATEWAY),
    };
    if let Some(title) = requested_title {
        let _ = sqlx::query("UPDATE feeds SET title=? WHERE id=?")
            .bind(title)
            .bind(id)
            .execute(&s.pool)
            .await;
    }
    if let Ok(feed) = sqlx::query_as::<_, crate::models::Feed>("SELECT * FROM feeds WHERE id=?")
        .bind(id)
        .fetch_one(&s.pool)
        .await
    {
        let fetcher = s.fetcher.clone();
        let pool = s.pool.clone();
        tokio::spawn(async move {
            let _ = fetcher.refresh_feed(&pool, &feed).await;
        });
    }
    Ok(Json(serde_json::json!({"id":id})))
}
#[derive(Deserialize)]
struct FeedUpdate {
    title: Option<String>,
    folder: Option<String>,
    paused: Option<bool>,
}
async fn update_feed(
    State(s): State<Arc<AppState>>,
    ConnectInfo(a): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<FeedUpdate>,
) -> ApiResult<StatusCode> {
    let session = auth(&s, &h, a).await?;
    csrf(&s, &h, session)?;
    let r = sqlx::query(
        "UPDATE feeds SET title=COALESCE(?,title),folder=?,paused=COALESCE(?,paused) WHERE id=?",
    )
    .bind(body.title)
    .bind(body.folder)
    .bind(body.paused)
    .bind(id)
    .execute(&s.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if r.rows_affected() == 0 {
        Err(StatusCode::NOT_FOUND)
    } else {
        Ok(StatusCode::NO_CONTENT)
    }
}
async fn delete_feed(
    State(s): State<Arc<AppState>>,
    ConnectInfo(a): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let session = auth(&s, &h, a).await?;
    csrf(&s, &h, session)?;
    sqlx::query("DELETE FROM feeds WHERE id=?")
        .bind(id)
        .execute(&s.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn refresh_feed(
    State(s): State<Arc<AppState>>,
    ConnectInfo(a): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<axum::response::Response> {
    let session = auth(&s, &h, a).await?;
    csrf(&s, &h, session)?;
    let feed = sqlx::query_as::<_, crate::models::Feed>("SELECT * FROM feeds WHERE id=?")
        .bind(id)
        .fetch_optional(&s.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    match s.fetcher.refresh_feed(&s.pool, &feed).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT.into_response()),
        Err(e) => Ok((StatusCode::BAD_GATEWAY, e.to_string()).into_response()),
    }
}
async fn export_opml(
    State(s): State<Arc<AppState>>,
    ConnectInfo(a): ConnectInfo<SocketAddr>,
    h: HeaderMap,
) -> ApiResult<impl IntoResponse> {
    auth(&s, &h, a).await?;
    let rows = sqlx::query_as::<_, (String, String, Option<String>)>(
        "SELECT title,url,folder FROM feeds ORDER BY folder,title",
    )
    .fetch_all(&s.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(([("content-type", "text/x-opml")], opml::export_opml(&rows)))
}
async fn import_opml(
    State(s): State<Arc<AppState>>,
    ConnectInfo(a): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    body: String,
) -> ApiResult<Json<serde_json::Value>> {
    let session = auth(&s, &h, a).await?;
    csrf(&s, &h, session)?;
    let feeds = opml::parse_opml(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    let mut added = 0;
    for (title, url, folder) in feeds {
        let Ok(url) = validate_url(
            &url,
            s.config.feed.allow_private_networks,
            &s.config.feed.allowed_hosts,
        ) else {
            continue;
        };
        if db::add_feed(&s.pool, &title, url.as_str(), folder.as_deref())
            .await
            .is_ok()
        {
            added += 1
        }
    }
    Ok(Json(serde_json::json!({"imported":added})))
}

async fn list_credentials(
    State(s): State<Arc<AppState>>,
    ConnectInfo(a): ConnectInfo<SocketAddr>,
    h: HeaderMap,
) -> ApiResult<Json<Vec<AppCredentialView>>> {
    auth(&s, &h, a).await?;
    sqlx::query_as::<_, AppCredentialView>(
        "SELECT id,name,username,enabled,created_at,last_used_at FROM app_credentials ORDER BY id",
    )
    .fetch_all(&s.pool)
    .await
    .map(Json)
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
#[derive(Deserialize)]
struct CredIn {
    name: String,
}
async fn create_credential(
    State(s): State<Arc<AppState>>,
    ConnectInfo(a): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Json(body): Json<CredIn>,
) -> ApiResult<Json<AppCredentialCreated>> {
    let session = auth(&s, &h, a).await?;
    csrf(&s, &h, session)?;
    if body.name.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let username = format!("app-{}", db::random_token(8).to_lowercase());
    let password = db::random_token(24);
    let hash = Argon2::default()
        .hash_password(
            password.as_bytes(),
            &SaltString::generate(&mut rand_core::OsRng),
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .to_string();
    let fever = format!("{:x}", Md5::digest(format!("{username}:{password}")));
    let fever_hash = Argon2::default()
        .hash_password(
            fever.as_bytes(),
            &SaltString::generate(&mut rand_core::OsRng),
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .to_string();
    let id:i64=sqlx::query_scalar("INSERT INTO app_credentials(name,username,password_hash,fever_hash) VALUES (?,?,?,?) RETURNING id").bind(&body.name).bind(&username).bind(hash).bind(fever_hash).fetch_one(&s.pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(AppCredentialCreated {
        id,
        name: body.name,
        username,
        password,
    }))
}
async fn revoke_credential(
    State(s): State<Arc<AppState>>,
    ConnectInfo(a): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let session = auth(&s, &h, a).await?;
    csrf(&s, &h, session)?;
    sqlx::query("UPDATE app_credentials SET enabled=0 WHERE id=?")
        .bind(id)
        .execute(&s.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    sqlx::query("DELETE FROM api_tokens WHERE credential_id=?")
        .bind(id)
        .execute(&s.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct BackupIn {
    pub path: String,
}
async fn backup(
    State(s): State<Arc<AppState>>,
    ConnectInfo(a): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Json(body): Json<BackupIn>,
) -> ApiResult<StatusCode> {
    let session = auth(&s, &h, a).await?;
    csrf(&s, &h, session)?;
    if !body
        .path
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        || !body.path.ends_with(".db")
        || body.path == ".db"
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let path = s.config.data_dir.join(body.path);
    db::backup(&s.pool, &path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn verify_app_password(
    state: &AppState,
    username: &str,
    password: &str,
) -> Result<i64, StatusCode> {
    let row =
        sqlx::query("SELECT id,password_hash FROM app_credentials WHERE username=? AND enabled=1")
            .bind(username)
            .fetch_optional(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::UNAUTHORIZED)?;
    let id: i64 = row.get("id");
    let hash: String = row.get("password_hash");
    let parsed = PasswordHash::new(&hash).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    let _ = sqlx::query("UPDATE app_credentials SET last_used_at=CURRENT_TIMESTAMP WHERE id=?")
        .bind(id)
        .execute(&state.pool)
        .await;
    Ok(id)
}
pub async fn verify_fever_key(state: &AppState, key: &str) -> Result<i64, StatusCode> {
    let rows = sqlx::query("SELECT id,fever_hash FROM app_credentials WHERE enabled=1")
        .fetch_all(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    for row in rows {
        let hash: String = row.get("fever_hash");
        if let Ok(parsed) = PasswordHash::new(&hash) {
            if Argon2::default()
                .verify_password(key.as_bytes(), &parsed)
                .is_ok()
            {
                return Ok(row.get("id"));
            }
        }
    }
    Err(StatusCode::UNAUTHORIZED)
}
