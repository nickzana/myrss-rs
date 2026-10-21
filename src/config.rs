use serde::Deserialize;
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "default_listen")]
    pub listen: SocketAddr,
    pub public_url: String,
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub feed: FeedConfig,
    #[serde(default)]
    pub server: ServerConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct AuthConfig {
    #[serde(default)]
    pub mode: AuthMode,
    pub local: Option<LocalAuth>,
    pub oidc: Option<OidcAuth>,
    pub proxy: Option<ProxyAuth>,
    #[serde(default = "default_session_hours")]
    pub session_idle_hours: i64,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum AuthMode {
    #[default]
    Local,
    Oidc,
    Proxy,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalAuth {
    pub password_file: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OidcAuth {
    pub issuer_url: String,
    pub client_id: String,
    pub client_secret_file: Option<PathBuf>,
    pub subject: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProxyAuth {
    pub trusted_peer_cidrs: Vec<String>,
    pub user_header: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedConfig {
    #[serde(default = "default_refresh_minutes")]
    pub refresh_minutes: u64,
    #[serde(default = "default_max_feed_bytes")]
    pub max_feed_bytes: usize,
    #[serde(default = "default_max_article_bytes")]
    pub max_article_bytes: usize,
    #[serde(default = "default_request_timeout_seconds")]
    pub request_timeout_seconds: u64,
    #[serde(default)]
    pub allow_private_networks: bool,
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
}
impl Default for FeedConfig {
    fn default() -> Self {
        Self {
            refresh_minutes: default_refresh_minutes(),
            max_feed_bytes: default_max_feed_bytes(),
            max_article_bytes: default_max_article_bytes(),
            request_timeout_seconds: default_request_timeout_seconds(),
            allow_private_networks: false,
            allowed_hosts: vec![],
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    #[serde(default)]
    pub remote_media: bool,
    #[serde(default)]
    pub debug: bool,
    pub retention_days: Option<i64>,
}

fn default_listen() -> SocketAddr {
    "127.0.0.1:8080".parse().unwrap()
}
fn default_data_dir() -> PathBuf {
    PathBuf::from("./data")
}
fn default_session_hours() -> i64 {
    12
}
fn default_refresh_minutes() -> u64 {
    30
}
fn default_max_feed_bytes() -> usize {
    8 * 1024 * 1024
}
fn default_max_article_bytes() -> usize {
    1024 * 1024
}
fn default_request_timeout_seconds() -> u64 {
    30
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let mut cfg: Self =
            toml::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))?;
        if let Ok(v) = std::env::var("MYRSS_PUBLIC_URL") {
            cfg.public_url = v;
        }
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<(), String> {
        let public_url =
            url::Url::parse(&self.public_url).map_err(|_| "public_url must be an absolute URL")?;
        if !matches!(public_url.scheme(), "http" | "https") {
            return Err("public_url must use http or https".into());
        }
        if self.auth.session_idle_hours <= 0 {
            return Err("auth.session_idle_hours must be greater than zero".into());
        }
        if self.feed.request_timeout_seconds == 0 {
            return Err("feed.request_timeout_seconds must be greater than zero".into());
        }
        if self.feed.refresh_minutes == 0 {
            return Err("feed.refresh_minutes must be greater than zero".into());
        }
        if self.feed.max_feed_bytes == 0 || self.feed.max_feed_bytes > 64 * 1024 * 1024 {
            return Err("feed.max_feed_bytes must be 1..=64MiB".into());
        }
        if self.feed.max_article_bytes == 0 || self.feed.max_article_bytes > 8 * 1024 * 1024 {
            return Err("feed.max_article_bytes must be 1..=8MiB".into());
        }
        match self.auth.mode {
            AuthMode::Local => {
                let local = self
                    .auth
                    .local
                    .as_ref()
                    .ok_or("auth.local is required in local mode")?;
                let meta = std::fs::metadata(&local.password_file)
                    .map_err(|e| format!("auth.local.password_file: {e}"))?;
                if meta.len() < 12 {
                    return Err("auth.local.password_file must contain at least 12 bytes".into());
                }
            }
            AuthMode::Oidc => {
                let oidc = self
                    .auth
                    .oidc
                    .as_ref()
                    .ok_or("auth.oidc is required in oidc mode")?;
                let issuer = url::Url::parse(&oidc.issuer_url)
                    .map_err(|_| "auth.oidc.issuer_url must be absolute")?;
                if !matches!(issuer.scheme(), "http" | "https") {
                    return Err("auth.oidc.issuer_url must use http or https".into());
                }
                if oidc.client_id.is_empty() {
                    return Err("auth.oidc.client_id is required".into());
                }
                if let Some(path) = &oidc.client_secret_file {
                    std::fs::metadata(path)
                        .map_err(|e| format!("auth.oidc.client_secret_file: {e}"))?;
                }
            }
            AuthMode::Proxy => {
                let proxy = self
                    .auth
                    .proxy
                    .as_ref()
                    .ok_or("auth.proxy is required in proxy mode")?;
                if proxy.trusted_peer_cidrs.is_empty() {
                    return Err("auth.proxy.trusted_peer_cidrs cannot be empty".into());
                }
                for cidr in &proxy.trusted_peer_cidrs {
                    cidr.parse::<ipnet::IpNet>()
                        .map_err(|_| format!("invalid proxy CIDR: {cidr}"))?;
                }
                if proxy.user_header.trim().is_empty() {
                    return Err("proxy user_header is required".into());
                }
            }
        }
        Ok(())
    }

    pub fn database_path(&self) -> PathBuf {
        self.data_dir.join("myrss.db")
    }
}
