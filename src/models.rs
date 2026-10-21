use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Feed {
    pub id: i64,
    pub title: String,
    pub url: String,
    pub site_url: Option<String>,
    pub folder: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    #[serde(skip_serializing)]
    pub http_auth: Option<String>,
    pub last_error: Option<String>,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub paused: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FeedSummary {
    pub id: i64,
    pub title: String,
    pub url: String,
    pub site_url: Option<String>,
    pub folder: Option<String>,
    pub unread: i64,
    pub last_error: Option<String>,
    pub paused: bool,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct EntryListItem {
    pub id: i64,
    pub feed_id: i64,
    pub feed_title: String,
    pub folder: Option<String>,
    pub title: String,
    pub author: Option<String>,
    pub url: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
    pub discovered_at: DateTime<Utc>,
    pub read: bool,
    pub starred: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct EntryDetail {
    #[serde(flatten)]
    pub item: EntryListItem,
    pub summary: Option<String>,
    pub content: Option<String>,
    pub labels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryState {
    pub read: Option<bool>,
    pub starred: Option<bool>,
    pub labels: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct AppCredentialView {
    pub id: i64,
    pub name: String,
    pub username: String,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppCredentialCreated {
    pub id: i64,
    pub name: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub feeds: Vec<FeedSummary>,
    pub folders: Vec<String>,
    pub labels: Vec<String>,
    pub unread: i64,
    pub starred: i64,
    pub total: i64,
    pub auth_mode: String,
    pub remote_media: bool,
}
