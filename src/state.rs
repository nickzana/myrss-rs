use crate::{
    config::{AuthMode, Config},
    ingest::Fetcher,
};
use sqlx::SqlitePool;
use std::{collections::HashMap, net::IpAddr, sync::Mutex, time::Instant};

pub struct AppState {
    pub config: Config,
    pub pool: SqlitePool,
    pub fetcher: Fetcher,
    pub http: reqwest::Client,
    pub login_attempts: Mutex<HashMap<IpAddr, (u32, Instant)>>,
}

impl AppState {
    pub fn new(config: Config, pool: SqlitePool) -> Self {
        let fetcher = Fetcher::new(config.feed.clone(), config.server.remote_media);
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap_or_default();
        Self {
            config,
            pool,
            fetcher,
            http,
            login_attempts: Mutex::new(HashMap::new()),
        }
    }
    pub fn auth_mode(&self) -> &'static str {
        match self.config.auth.mode {
            AuthMode::Local => "local",
            AuthMode::Oidc => "oidc",
            AuthMode::Proxy => "proxy",
        }
    }
}
