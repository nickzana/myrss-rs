pub mod api;
pub mod config;
pub mod db;
pub mod ingest;
pub mod migrate;
pub mod models;
pub mod security;
pub mod state;

pub use state::AppState;

/// Tracing filter derived from config; RUST_LOG overrides this entirely.
pub fn log_filter(config: &config::Config) -> String {
    if config.server.debug {
        "myrss=debug,tower_http=debug,sqlx=warn".to_string()
    } else {
        "info".to_string()
    }
}
