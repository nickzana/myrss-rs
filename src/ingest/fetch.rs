use crate::{
    config::FeedConfig,
    db,
    models::Feed,
    security::{sanitize_html, validate_url},
};
use futures::StreamExt;
use reqwest::{header, Client, StatusCode};
use sqlx::SqlitePool;
use std::sync::Arc;
use thiserror::Error;
use url::Url;

fn error_chain(error: &dyn std::error::Error) -> String {
    let mut out = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        out.push_str(": ");
        out.push_str(&cause.to_string());
        source = cause.source();
    }
    out
}

#[derive(Debug, Error)]
pub enum IngestError {
    #[error("URL rejected: {0}")]
    Url(#[from] crate::security::ssrf::UrlGuardError),
    #[error("HTTP {0}")]
    Http(StatusCode),
    #[error("request failed: {}", error_chain(.0))]
    Request(#[from] reqwest::Error),
    #[error("feed parse failed")]
    Parse,
    #[error("database failed: {0}")]
    Db(#[from] sqlx::Error),
}

#[derive(Debug, Clone)]
pub struct Fetcher {
    client: Client,
    config: FeedConfig,
    remote_media: bool,
    refresh_lock: Arc<tokio::sync::Mutex<()>>,
}

impl Fetcher {
    pub fn new(config: FeedConfig, remote_media: bool) -> Self {
        let resolver = crate::security::ssrf::PublicDnsResolver::new(
            config.allow_private_networks,
            config.allowed_hosts.clone(),
        );
        let mut builder = Client::builder().dns_resolver(Arc::new(resolver));
        if let Some(proxy) = &config.proxy {
            builder = builder
                .proxy(reqwest::Proxy::all(proxy).expect("feed.proxy must be a valid proxy URL"));
        }
        let client = builder
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(
                config.request_timeout_seconds,
            ))
            .user_agent("myrss/0.1 (+https://github.com/nickzana/myrss-rs)")
            .build()
            .expect("feed HTTP client configuration must be valid");
        Self {
            client,
            config,
            remote_media,
            refresh_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub async fn refresh_due(&self, pool: &SqlitePool) -> usize {
        let feeds = match db::due_feeds(pool).await {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("due feed query failed: {e}");
                return 0;
            }
        };
        let count = feeds.len();
        futures::stream::iter(feeds)
            .map(|feed| async move {
                if let Err(e) = self.refresh_feed(pool, &feed).await {
                    tracing::warn!(feed=%feed.url,error=%e,"refresh failed");
                }
            })
            .buffer_unordered(8)
            .collect::<Vec<_>>()
            .await;
        count
    }

    pub async fn refresh_feed(&self, pool: &SqlitePool, feed: &Feed) -> Result<(), IngestError> {
        let _refresh = self.refresh_lock.lock().await;
        match self.refresh_feed_inner(pool, feed).await {
            Ok(()) => Ok(()),
            Err(error) => {
                let _ = db::update_feed_http(
                    pool,
                    feed.id,
                    None,
                    None,
                    Some(&error.to_string()),
                    self.config.refresh_minutes as i64,
                )
                .await;
                Err(error)
            }
        }
    }

    async fn refresh_feed_inner(&self, pool: &SqlitePool, feed: &Feed) -> Result<(), IngestError> {
        let response = self
            .fetch(
                &feed.url,
                feed.etag.as_deref(),
                feed.last_modified.as_deref(),
                feed.http_auth.as_deref(),
            )
            .await?;

        match response {
            FetchResult::NotModified { etag, modified } => {
                db::update_feed_http(
                    pool,
                    feed.id,
                    etag.as_deref(),
                    modified.as_deref(),
                    None,
                    self.config.refresh_minutes as i64,
                )
                .await?
            }
            FetchResult::Body {
                url,
                bytes,
                etag,
                modified,
                content_type,
            } => {
                let (title, site_url, entries) =
                    super::parse::parse_feed(&bytes, url.as_str(), Some(content_type.as_str()))
                        .map_err(|_| IngestError::Parse)?;
                if title != feed.title || site_url != feed.site_url {
                    sqlx::query("UPDATE feeds SET title=?,site_url=? WHERE id=?")
                        .bind(&title)
                        .bind(&site_url)
                        .bind(feed.id)
                        .execute(pool)
                        .await?;
                }
                for item in entries.into_iter().take(10_000) {
                    let content = item.content.map(|mut v| {
                        truncate_utf8(&mut v, self.config.max_article_bytes);
                        sanitize_html(
                            &v,
                            item.url
                                .as_deref()
                                .or(site_url.as_deref())
                                .or(Some(url.as_str())),
                            self.remote_media,
                        )
                    });
                    let summary = item.summary.map(|mut v| {
                        truncate_utf8(&mut v, self.config.max_article_bytes);
                        sanitize_html(
                            &v,
                            item.url
                                .as_deref()
                                .or(site_url.as_deref())
                                .or(Some(url.as_str())),
                            self.remote_media,
                        )
                    });
                    db::upsert_entry(
                        pool,
                        feed.id,
                        &item.guid,
                        item.url.as_deref(),
                        &item.title,
                        item.author.as_deref(),
                        summary.as_deref(),
                        content.as_deref(),
                        item.published_at,
                    )
                    .await?;
                }
                db::update_feed_http(
                    pool,
                    feed.id,
                    etag.as_deref(),
                    modified.as_deref(),
                    None,
                    self.config.refresh_minutes as i64,
                )
                .await?;
            }
        }
        Ok(())
    }

    pub async fn discover_and_add(
        &self,
        pool: &SqlitePool,
        raw_url: &str,
        folder: Option<&str>,
    ) -> Result<i64, IngestError> {
        let url = validate_url(
            raw_url,
            self.config.allow_private_networks,
            &self.config.allowed_hosts,
        )?;
        match self.fetch(url.as_str(), None, None, None).await? {
            FetchResult::Body {
                bytes,
                content_type,
                url: final_url,
                ..
            } => {
                match super::parse::parse_feed(
                    &bytes,
                    final_url.as_str(),
                    Some(content_type.as_str()),
                ) {
                    Ok((title, _, _)) => {
                        self.add_feed(pool, &title, final_url.as_str(), folder)
                            .await
                    }
                    Err(_) => {
                        let discovered = super::parse::discover_feed_url(
                            &String::from_utf8_lossy(&bytes),
                            &final_url,
                        )
                        .ok_or(IngestError::Parse)?;
                        let discovered = validate_url(
                            discovered.as_str(),
                            self.config.allow_private_networks,
                            &self.config.allowed_hosts,
                        )?;
                        match self.fetch(discovered.as_str(), None, None, None).await? {
                            FetchResult::Body {
                                bytes,
                                content_type,
                                url: feed_url,
                                ..
                            } => {
                                let (title, _, _) = super::parse::parse_feed(
                                    &bytes,
                                    feed_url.as_str(),
                                    Some(content_type.as_str()),
                                )
                                .map_err(|_| IngestError::Parse)?;
                                self.add_feed(pool, &title, feed_url.as_str(), folder).await
                            }
                            _ => Err(IngestError::Parse),
                        }
                    }
                }
            }
            _ => Err(IngestError::Parse),
        }
    }

    async fn add_feed(
        &self,
        pool: &SqlitePool,
        title: &str,
        url: &str,
        folder: Option<&str>,
    ) -> Result<i64, IngestError> {
        let _refresh = self.refresh_lock.lock().await;
        Ok(db::add_feed(pool, title, url, folder).await?)
    }

    async fn fetch(
        &self,
        raw: &str,
        etag: Option<&str>,
        modified: Option<&str>,
        http_auth: Option<&str>,
    ) -> Result<FetchResult, IngestError> {
        let mut current = validate_url(
            raw,
            self.config.allow_private_networks,
            &self.config.allowed_hosts,
        )?;
        let auth_host = current.host_str().unwrap_or_default().to_string();
        for redirect_count in 0..=5 {
            let mut req = self.client.get(current.clone());
            if redirect_count == 0 {
                if let Some(v) = etag {
                    req = req.header(header::IF_NONE_MATCH, v);
                }
                if let Some(v) = modified {
                    req = req.header(header::IF_MODIFIED_SINCE, v);
                }
            }
            if current.host_str() == Some(auth_host.as_str()) {
                if let Some(v) = http_auth {
                    let encoded =
                        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, v);
                    req = req.header(header::AUTHORIZATION, format!("Basic {encoded}"));
                }
            }
            let mut resp = req.send().await?;
            if resp.status() == StatusCode::NOT_MODIFIED {
                return Ok(FetchResult::NotModified {
                    etag: header_string(&resp, header::ETAG),
                    modified: header_string(&resp, header::LAST_MODIFIED),
                });
            }
            if resp.status().is_redirection() {
                let loc = resp
                    .headers()
                    .get(header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or(IngestError::Http(resp.status()))?;
                current = validate_url(
                    current.join(loc).map_err(|_| IngestError::Parse)?.as_str(),
                    self.config.allow_private_networks,
                    &self.config.allowed_hosts,
                )?;
                continue;
            }
            if !resp.status().is_success() {
                return Err(IngestError::Http(resp.status()));
            }
            let ctype = resp
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            let etag = header_string(&resp, header::ETAG);
            let modified = header_string(&resp, header::LAST_MODIFIED);
            let mut bytes = Vec::new();
            while let Some(chunk) = resp.chunk().await? {
                if bytes.len() + chunk.len() > self.config.max_feed_bytes {
                    return Err(IngestError::Http(StatusCode::PAYLOAD_TOO_LARGE));
                }
                bytes.extend_from_slice(&chunk);
            }
            return Ok(FetchResult::Body {
                url: current,
                bytes,
                etag,
                modified,
                content_type: ctype,
            });
        }
        Err(IngestError::Http(StatusCode::LOOP_DETECTED))
    }
}

fn header_string(resp: &reqwest::Response, name: header::HeaderName) -> Option<String> {
    resp.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

pub enum FetchResult {
    NotModified {
        etag: Option<String>,
        modified: Option<String>,
    },
    Body {
        url: Url,
        bytes: Vec<u8>,
        etag: Option<String>,
        modified: Option<String>,
        content_type: String,
    },
}

fn truncate_utf8(value: &mut String, max: usize) {
    if value.len() <= max {
        return;
    }
    let mut end = max;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
}
