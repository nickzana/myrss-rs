use crate::{config::Config, models::*};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    Row, SqlitePool,
};
use std::path::Path;

pub type DbResult<T> = Result<T, sqlx::Error>;

pub async fn connect(config: &Config) -> DbResult<SqlitePool> {
    std::fs::create_dir_all(&config.data_dir).map_err(sqlx::Error::Io)?;
    let options = SqliteConnectOptions::new()
        .filename(config.database_path())
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Full)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    sqlx::query("INSERT OR IGNORE INTO owner(id, username) VALUES (1, 'owner')")
        .execute(&pool)
        .await?;
    Ok(pool)
}

pub fn token_hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

pub async fn create_session(pool: &SqlitePool, idle_hours: i64) -> DbResult<(String, String)> {
    let token = random_token(32);
    let csrf = random_token(24);
    let expires = Utc::now() + Duration::hours(idle_hours);
    sqlx::query("INSERT INTO sessions(token_hash, csrf_token, expires_at) VALUES (?, ?, ?)")
        .bind(token_hash(&token))
        .bind(&csrf)
        .bind(expires)
        .execute(pool)
        .await?;
    Ok((token, csrf))
}

pub async fn validate_session(
    pool: &SqlitePool,
    token: &str,
    idle_hours: i64,
) -> DbResult<Option<String>> {
    sqlx::query("DELETE FROM sessions WHERE expires_at < ?")
        .bind(Utc::now())
        .execute(pool)
        .await?;
    let row = sqlx::query("SELECT csrf_token, expires_at FROM sessions WHERE token_hash = ?")
        .bind(token_hash(token))
        .fetch_optional(pool)
        .await?;
    if let Some(row) = row {
        let renewed = Utc::now() + Duration::hours(idle_hours);
        sqlx::query("UPDATE sessions SET expires_at = ? WHERE token_hash = ?")
            .bind(renewed)
            .bind(token_hash(token))
            .execute(pool)
            .await?;
        Ok(Some(row.get("csrf_token")))
    } else {
        Ok(None)
    }
}

pub async fn logout(pool: &SqlitePool, token: &str) -> DbResult<()> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
        .bind(token_hash(token))
        .execute(pool)
        .await?;
    Ok(())
}

pub fn random_token(bytes: usize) -> String {
    let mut data = vec![0u8; bytes];
    rand::fill(&mut data[..]);
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, data)
}

pub async fn list_feeds(pool: &SqlitePool) -> DbResult<Vec<FeedSummary>> {
    sqlx::query_as(
        r#"SELECT f.id,f.title,f.url,f.site_url,f.folder,f.last_error,f.paused,
      COALESCE(SUM(CASE WHEN s.read = 0 THEN 1 ELSE 0 END),0) AS unread
      FROM feeds f LEFT JOIN entries e ON e.feed_id=f.id LEFT JOIN entry_state s ON s.entry_id=e.id
      GROUP BY f.id ORDER BY lower(f.title)"#,
    )
    .fetch_all(pool)
    .await
}

pub async fn get_feed_by_url(pool: &SqlitePool, url: &str) -> DbResult<Option<Feed>> {
    sqlx::query_as("SELECT * FROM feeds WHERE url = ?")
        .bind(url)
        .fetch_optional(pool)
        .await
}

pub async fn add_feed(
    pool: &SqlitePool,
    title: &str,
    url: &str,
    folder: Option<&str>,
) -> DbResult<i64> {
    let r = sqlx::query("INSERT INTO feeds(title,url,folder) VALUES (?,?,?) ON CONFLICT(url) DO UPDATE SET title=excluded.title, folder=COALESCE(excluded.folder, feeds.folder) RETURNING id")
        .bind(title).bind(url).bind(folder).fetch_one(pool).await?;
    Ok(r.get(0))
}

pub async fn update_feed_http(
    pool: &SqlitePool,
    id: i64,
    etag: Option<&str>,
    modified: Option<&str>,
    error: Option<&str>,
    retry_minutes: i64,
) -> DbResult<()> {
    let retry = error.map(|_| Utc::now() + Duration::minutes(retry_minutes));
    sqlx::query("UPDATE feeds SET etag=COALESCE(?,etag), last_modified=COALESCE(?,last_modified), last_error=?, next_retry_at=? WHERE id=?")
        .bind(etag).bind(modified).bind(error).bind(retry).bind(id).execute(pool).await?;
    Ok(())
}

pub async fn due_feeds(pool: &SqlitePool) -> DbResult<Vec<Feed>> {
    sqlx::query_as("SELECT * FROM feeds WHERE paused = 0 AND (next_retry_at IS NULL OR next_retry_at <= ?) ORDER BY id").bind(Utc::now()).fetch_all(pool).await
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert_entry(
    pool: &SqlitePool,
    feed_id: i64,
    guid: &str,
    url: Option<&str>,
    title: &str,
    author: Option<&str>,
    summary: Option<&str>,
    content: Option<&str>,
    published: Option<DateTime<Utc>>,
) -> DbResult<i64> {
    let now = Utc::now();
    let mut tx = pool.begin().await?;
    if sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM deleted_entries WHERE feed_id=? AND guid=?",
    )
    .bind(feed_id)
    .bind(guid)
    .fetch_one(&mut *tx)
    .await?
        > 0
    {
        tx.commit().await?;
        return Ok(0);
    }
    sqlx::query("INSERT INTO entries(feed_id,guid,url,title,author,summary,content,published_at,discovered_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?,?) ON CONFLICT(feed_id,guid) DO UPDATE SET url=excluded.url,title=excluded.title,author=excluded.author,summary=excluded.summary,content=excluded.content,published_at=excluded.published_at,updated_at=excluded.updated_at")
        .bind(feed_id).bind(guid).bind(url).bind(title).bind(author).bind(summary).bind(content).bind(published).bind(now).bind(now).execute(&mut *tx).await?;
    let id: i64 = sqlx::query_scalar("SELECT id FROM entries WHERE feed_id=? AND guid=?")
        .bind(feed_id)
        .bind(guid)
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query("INSERT OR IGNORE INTO entry_state(entry_id) VALUES (?)")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM entries_fts WHERE rowid = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO entries_fts(rowid,title,author,summary,content) VALUES (?,?,?,?,?)")
        .bind(id)
        .bind(title)
        .bind(author.unwrap_or(""))
        .bind(summary.unwrap_or(""))
        .bind(content.unwrap_or(""))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(id)
}

#[derive(Debug, Clone, Deserialize)]
pub struct EntryQuery {
    pub filter: Option<String>,
    pub feed_id: Option<i64>,
    pub folder: Option<String>,
    pub label: Option<String>,
    pub q: Option<String>,
    pub before: Option<i64>,
    pub limit: Option<i64>,
    pub unread: Option<bool>,
    pub starred: Option<bool>,
}

pub async fn list_entries(pool: &SqlitePool, q: &EntryQuery) -> DbResult<Vec<EntryListItem>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let mut sql = String::from(
        r#"SELECT e.id,e.feed_id,f.title AS feed_title,f.folder,e.title,e.author,e.url,e.published_at,e.discovered_at,s.read,s.starred
      FROM entries e JOIN feeds f ON f.id=e.feed_id JOIN entry_state s ON s.entry_id=e.id
      LEFT JOIN entry_labels el ON el.entry_id=e.id LEFT JOIN labels l ON l.id=el.label_id WHERE 1=1"#,
    );
    let mut args: Vec<String> = vec![];
    let filter = q.filter.as_deref().unwrap_or("all");
    match filter {
        "unread" => sql.push_str(" AND s.read=0"),
        "starred" => sql.push_str(" AND s.starred=1"),
        "read" => sql.push_str(" AND s.read=1"),
        _ => {}
    }
    if let Some(v) = q.feed_id {
        sql.push_str(" AND e.feed_id=?");
        args.push(v.to_string());
    }
    if let Some(v) = &q.folder {
        sql.push_str(" AND f.folder=?");
        args.push(v.clone());
    }
    if let Some(v) = &q.label {
        sql.push_str(" AND l.name=?");
        args.push(v.clone());
    }
    if let Some(v) = q.unread {
        sql.push_str(if v { " AND s.read=0" } else { " AND s.read=1" });
    }
    if let Some(v) = q.starred {
        sql.push_str(if v {
            " AND s.starred=1"
        } else {
            " AND s.starred=0"
        });
    }
    if let Some(v) = &q.q {
        sql.push_str(" AND e.id IN (SELECT rowid FROM entries_fts WHERE entries_fts MATCH ?)");
        args.push(
            v.split_whitespace()
                .map(|w| format!("\"{}\"*", w.replace('"', " ")))
                .collect::<Vec<_>>()
                .join(" AND "),
        );
    }
    if let Some(v) = q.before {
        sql.push_str(" AND (COALESCE(e.published_at,e.discovered_at) < (SELECT COALESCE(published_at,discovered_at) FROM entries WHERE id=?) OR (COALESCE(e.published_at,e.discovered_at) = (SELECT COALESCE(published_at,discovered_at) FROM entries WHERE id=?) AND e.id < ?))");
        args.push(v.to_string());
        args.push(v.to_string());
        args.push(v.to_string());
    }
    sql.push_str(
        " GROUP BY e.id ORDER BY COALESCE(e.published_at,e.discovered_at) DESC,e.id DESC LIMIT ?",
    );
    args.push(limit.to_string());
    let mut query = sqlx::query_as::<_, EntryListItem>(&sql);
    for arg in args {
        query = query.bind(arg);
    }
    query.fetch_all(pool).await
}

pub async fn get_entry(pool: &SqlitePool, id: i64) -> DbResult<Option<EntryDetail>> {
    let item = sqlx::query_as::<_,EntryListItem>(r#"SELECT e.id,e.feed_id,f.title AS feed_title,f.folder,e.title,e.author,e.url,e.published_at,e.discovered_at,s.read,s.starred
      FROM entries e JOIN feeds f ON f.id=e.feed_id JOIN entry_state s ON s.entry_id=e.id WHERE e.id=?"#).bind(id).fetch_optional(pool).await?;
    let Some(item) = item else { return Ok(None) };
    let row = sqlx::query("SELECT summary,content FROM entries WHERE id=?")
        .bind(id)
        .fetch_one(pool)
        .await?;
    let labels = sqlx::query_scalar::<_,String>("SELECT l.name FROM labels l JOIN entry_labels el ON el.label_id=l.id WHERE el.entry_id=? ORDER BY l.name").bind(id).fetch_all(pool).await?;
    Ok(Some(EntryDetail {
        item,
        summary: row.get(0),
        content: row.get(1),
        labels,
    }))
}

pub async fn set_entry_state(pool: &SqlitePool, id: i64, state: &EntryState) -> DbResult<()> {
    if let Some(read) = state.read {
        sqlx::query("UPDATE entry_state SET read=?, read_at=CASE WHEN ? THEN CURRENT_TIMESTAMP ELSE NULL END, updated_at=CURRENT_TIMESTAMP WHERE entry_id=?").bind(read).bind(read).bind(id).execute(pool).await?;
    }
    if let Some(starred) = state.starred {
        sqlx::query(
            "UPDATE entry_state SET starred=?, updated_at=CURRENT_TIMESTAMP WHERE entry_id=?",
        )
        .bind(starred)
        .bind(id)
        .execute(pool)
        .await?;
    }
    if let Some(labels) = &state.labels {
        let mut tx = pool.begin().await?;
        sqlx::query("DELETE FROM entry_labels WHERE entry_id=?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        for name in labels {
            sqlx::query("INSERT OR IGNORE INTO labels(name) VALUES (?)")
                .bind(name)
                .execute(&mut *tx)
                .await?;
            sqlx::query("INSERT OR IGNORE INTO entry_labels(entry_id,label_id) SELECT ?,id FROM labels WHERE name=?").bind(id).bind(name).execute(&mut *tx).await?;
        }
        tx.commit().await?;
    }
    Ok(())
}

pub async fn mark_all_read(pool: &SqlitePool, feed_id: Option<i64>) -> DbResult<u64> {
    let r = if let Some(id) = feed_id {
        sqlx::query("UPDATE entry_state SET read=1,read_at=CURRENT_TIMESTAMP,updated_at=CURRENT_TIMESTAMP WHERE entry_id IN (SELECT id FROM entries WHERE feed_id=?)").bind(id).execute(pool).await?
    } else {
        sqlx::query(
            "UPDATE entry_state SET read=1,read_at=CURRENT_TIMESTAMP,updated_at=CURRENT_TIMESTAMP",
        )
        .execute(pool)
        .await?
    };
    Ok(r.rows_affected())
}

pub async fn mark_label_read(pool: &SqlitePool, label: &str) -> DbResult<u64> {
    let r = sqlx::query("UPDATE entry_state SET read=1,read_at=CURRENT_TIMESTAMP,updated_at=CURRENT_TIMESTAMP WHERE entry_id IN (SELECT e.id FROM entries e JOIN feeds f ON f.id=e.feed_id LEFT JOIN entry_labels el ON el.entry_id=e.id LEFT JOIN labels l ON l.id=el.label_id WHERE f.folder=? OR l.name=?)")
        .bind(label)
        .bind(label)
        .execute(pool)
        .await?;
    Ok(r.rows_affected())
}

pub async fn snapshot(
    pool: &SqlitePool,
    auth_mode: &str,
    remote_media: bool,
) -> DbResult<Snapshot> {
    let feeds = list_feeds(pool).await?;
    let folders = sqlx::query_scalar(
        "SELECT DISTINCT folder FROM feeds WHERE folder IS NOT NULL ORDER BY folder",
    )
    .fetch_all(pool)
    .await?;
    let labels = sqlx::query_scalar("SELECT name FROM labels ORDER BY name")
        .fetch_all(pool)
        .await?;
    let row=sqlx::query("SELECT COALESCE(SUM(read=0),0) unread,COALESCE(SUM(starred),0) starred,COUNT(*) total FROM entry_state").fetch_one(pool).await?;
    Ok(Snapshot {
        feeds,
        folders,
        labels,
        unread: row.get(0),
        starred: row.get(1),
        total: row.get(2),
        auth_mode: auth_mode.into(),
        remote_media,
    })
}

pub async fn backup(pool: &SqlitePool, destination: &Path) -> DbResult<()> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(sqlx::Error::Io)?;
    }
    sqlx::query("VACUUM INTO ?")
        .bind(destination.display().to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn retain(pool: &SqlitePool, days: i64) -> DbResult<u64> {
    let cutoff = Utc::now() - Duration::days(days);
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT OR IGNORE INTO deleted_entries(feed_id,guid) SELECT feed_id,guid FROM entries e JOIN entry_state s ON s.entry_id=e.id WHERE s.read=1 AND s.starred=0 AND NOT EXISTS (SELECT 1 FROM entry_labels el WHERE el.entry_id=e.id) AND COALESCE(e.published_at,e.discovered_at) < ?",
    ).bind(cutoff).execute(&mut *tx).await?;
    let result = sqlx::query(
        "DELETE FROM entries WHERE id IN (SELECT e.id FROM entries e JOIN entry_state s ON s.entry_id=e.id WHERE s.read=1 AND s.starred=0 AND NOT EXISTS (SELECT 1 FROM entry_labels el WHERE el.entry_id=e.id) AND COALESCE(e.published_at,e.discovered_at) < ?)",
    ).bind(cutoff).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(result.rows_affected())
}

pub async fn add_entry_label(pool: &SqlitePool, entry_id: i64, name: &str) -> DbResult<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT OR IGNORE INTO labels(name) VALUES (?)")
        .bind(name)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT OR IGNORE INTO entry_labels(entry_id,label_id) SELECT ?,id FROM labels WHERE name=?").bind(entry_id).bind(name).execute(&mut *tx).await?;
    tx.commit().await
}

pub async fn remove_entry_label(pool: &SqlitePool, entry_id: i64, name: &str) -> DbResult<()> {
    sqlx::query("DELETE FROM entry_labels WHERE entry_id=? AND label_id IN (SELECT id FROM labels WHERE name=?)").bind(entry_id).bind(name).execute(pool).await?;
    Ok(())
}
