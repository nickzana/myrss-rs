pub mod auth;
pub mod fever;
pub mod greader;
pub mod native;

use axum::{
    http::{header, HeaderValue},
    response::{Html, IntoResponse},
    Router,
};
use std::sync::Arc;
use tower_http::{limit::RequestBodyLimitLayer, set_header::SetResponseHeaderLayer};

pub fn router(state: Arc<crate::AppState>) -> Router {
    Router::new()
        .route("/", axum::routing::get(index))
        .route("/app.js", axum::routing::get(js))
        .route("/style.css", axum::routing::get(css))
        .route("/health", axum::routing::get(|| async { "ok" }))
        .route("/ready", axum::routing::get(ready))
        .nest("/api/v1", native::router())
        .merge(auth::router())
        .nest("/api/greader.php", greader::router())
        .route(
            "/api/fever.php",
            axum::routing::post(fever::handle).get(fever::handle),
        )
        .layer(RequestBodyLimitLayer::new(16 * 1024 * 1024))
        .layer(SetResponseHeaderLayer::if_not_present(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data: https:; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'")))
        .layer(SetResponseHeaderLayer::if_not_present(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer")))
        .layer(SetResponseHeaderLayer::if_not_present(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")))
        .with_state(state)
}
async fn index() -> Html<&'static str> {
    Html(include_str!("../../static/index.html"))
}
async fn js() -> ([(header::HeaderName, HeaderValue); 1], &'static str) {
    (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/javascript"),
        )],
        include_str!("../../static/app.js"),
    )
}
async fn css() -> ([(header::HeaderName, HeaderValue); 1], &'static str) {
    (
        [(header::CONTENT_TYPE, HeaderValue::from_static("text/css"))],
        include_str!("../../static/style.css"),
    )
}
async fn ready(
    axum::extract::State(state): axum::extract::State<Arc<crate::AppState>>,
) -> impl IntoResponse {
    match sqlx::query("SELECT 1").execute(&state.pool).await {
        Ok(_) => axum::http::StatusCode::OK,
        Err(_) => axum::http::StatusCode::SERVICE_UNAVAILABLE,
    }
}
