use crate::{security::sanitize_html, AppState};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    Row, SqlitePool,
};
use std::{
    collections::HashSet,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize)]
pub struct ImportReport {
    pub feeds: u64,
    pub entries: u64,
    pub labels: u64,
    pub skipped_feeds: u64,
    pub skipped_entries: u64,
}

pub async fn import_freshrss(state: &AppState, source: &Path) -> Result<ImportReport, sqlx::Error> {
    let source = freshrss_database(source)?;
    let source_options = SqliteConnectOptions::new()
        .filename(&source)
        .read_only(true)
        .foreign_keys(false)
        .busy_timeout(std::time::Duration::from_secs(30));
    let source_pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(source_options)
        .await?;
    let table_names = table_names(&source_pool).await?;
    let prefix = freshrss_prefix(&table_names).ok_or(sqlx::Error::RowNotFound)?;
    let feed_table = format!("{prefix}feed");
    let entry_table = format!("{prefix}entry");
    let category_table = table_names
        .contains(&format!("{prefix}category"))
        .then(|| format!("{prefix}category"));
    let tag_table = table_names
        .contains(&format!("{prefix}tag"))
        .then(|| format!("{prefix}tag"));
    let entry_tag_table = table_names
        .contains(&format!("{prefix}entrytag"))
        .then(|| format!("{prefix}entrytag"));
    let feed_columns = columns(&source_pool, &feed_table).await?;
    let entry_columns = columns(&source_pool, &entry_table).await?;

    let mut report = ImportReport {
        feeds: 0,
        entries: 0,
        labels: 0,
        skipped_feeds: 0,
        skipped_entries: 0,
    };
    let feed_rows = sqlx::query(&format!(
        "SELECT id,url,name,{} website,{} category,{} httpAuth FROM {} ORDER BY id",
        if feed_columns.contains("website") {
            "website"
        } else {
            "NULL AS"
        },
        if feed_columns.contains("category") {
            "category"
        } else {
            "NULL AS"
        },
        if feed_columns.contains("httpAuth") {
            "httpAuth"
        } else {
            "NULL AS"
        },
        feed_table
    ))
    .fetch_all(&source_pool)
    .await?;
    let category_map = if let Some(table) = &category_table {
        sqlx::query(&format!("SELECT id,name FROM {table}"))
            .fetch_all(&source_pool)
            .await?
            .into_iter()
            .map(|r| (r.get::<i64, _>(0), r.get::<String, _>(1)))
            .collect::<std::collections::HashMap<_, _>>()
    } else {
        Default::default()
    };

    let mut feed_map = std::collections::HashMap::new();
    let mut tx = state.pool.begin().await?;
    for row in feed_rows {
        let source_id: i64 = row.get("id");
        let url: String = row.get("url");
        let name: String = row.get("name");
        let website: Option<String> = row.get("website");
        let category: Option<i64> = row.get("category");
        let http_auth: Option<String> = row.try_get("httpAuth").ok();
        if http_url(&url).is_none() {
            report.skipped_feeds += 1;
            continue;
        }
        if http_auth.as_ref().is_some_and(|v| !v.trim().is_empty()) {
            tracing::info!(feed=%name,"migrated FreshRSS feed HTTP credentials");
        }
        let folder = category.and_then(|id| category_map.get(&id).cloned());
        let site_url = website.and_then(|v| http_url(&v));
        let target = sqlx::query_scalar::<_, i64>(
            "INSERT INTO feeds(title,url,site_url,folder,http_auth) VALUES (?,?,?,?,?)
             ON CONFLICT(url) DO UPDATE SET title=excluded.title,site_url=excluded.site_url,folder=excluded.folder,http_auth=excluded.http_auth RETURNING id",
        )
        .bind(if name.trim().is_empty() { "Untitled feed" } else { &name })
        .bind(&url)
        .bind(&site_url)
        .bind(&folder)
        .bind(http_auth.as_deref().filter(|v| !v.trim().is_empty()))
        .fetch_one(&mut *tx)
        .await?;
        feed_map.insert(source_id, (target, site_url));
        report.feeds += 1;
    }
    tx.commit().await?;

    let content_expr = if entry_columns.contains("content") {
        "CAST(content AS BLOB)"
    } else {
        "content_bin"
    };
    let tags_expr = if entry_columns.contains("tags") {
        ",e.tags"
    } else {
        ",NULL AS tags"
    };
    let last_modified_expr = if entry_columns.contains("lastModified") {
        "e.`lastModified`"
    } else {
        "NULL"
    };
    let last_user_modified_expr = if entry_columns.contains("lastUserModified") {
        "e.`lastUserModified`"
    } else {
        "NULL"
    };
    let label_join = match (entry_tag_table, tag_table) {
        (Some(entry_tags), Some(tags)) => format!(
            "LEFT JOIN {entry_tags} et ON et.id_entry=e.id LEFT JOIN {tags} t ON t.id=et.id_tag"
        ),
        _ => String::new(),
    };
    let label_expr = if label_join.is_empty() {
        "NULL".to_string()
    } else {
        "GROUP_CONCAT(t.name,char(31))".to_string()
    };
    let entry_sql = format!(
        "SELECT e.id,e.guid,e.title,e.author,{content_expr} AS content,e.link,e.date,e.`lastSeen`,{last_modified_expr} AS lastModified,{last_user_modified_expr} AS lastUserModified,e.is_read,e.is_favorite,e.id_feed{tags_expr},{label_expr} AS labels FROM {entry_table} e {label_join} GROUP BY e.id ORDER BY e.id"
    );
    let mut rows = sqlx::query(&entry_sql).fetch(&source_pool);
    use futures::StreamExt;
    let mut pending = 0usize;
    let mut tx = state.pool.begin().await?;
    while let Some(row) = rows.next().await {
        let row = row?;
        let source_feed: i64 = row.get("id_feed");
        let Some((feed_id, site_url)) = feed_map.get(&source_feed).cloned() else {
            report.skipped_entries += 1;
            continue;
        };
        let source_id: i64 = row.get("id");
        let title = nonempty(row.get::<Option<String>, _>("title"), "Untitled");
        let author = empty_to_none(row.get::<Option<String>, _>("author"));
        let link = row
            .get::<Option<String>, _>("link")
            .and_then(|v| http_url(&v));
        let mut guid = row
            .get::<Option<String>, _>("guid")
            .unwrap_or_default()
            .trim()
            .to_string();
        if guid.is_empty() {
            guid = link
                .clone()
                .unwrap_or_else(|| format!("freshrss-{source_id}"));
        }
        let date =
            timestamp(row.try_get("date").ok()).or_else(|| timestamp(row.try_get("lastSeen").ok()));
        let discovered = timestamp(row.try_get("lastSeen").ok()).unwrap_or_else(Utc::now);
        let last_modified = timestamp(row.try_get("lastModified").ok());
        let last_user_modified = timestamp(row.try_get("lastUserModified").ok());
        let modified = last_user_modified.or(last_modified).unwrap_or(discovered);
        let content = row
            .get::<Option<Vec<u8>>, _>("content")
            .and_then(|v| decode_content(&v))
            .map(|v| {
                sanitize_html(
                    &v,
                    link.as_deref().or(site_url.as_deref()),
                    state.config.server.remote_media,
                )
            });
        let entry_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO entries(feed_id,guid,url,title,author,summary,content,published_at,discovered_at,updated_at)
             VALUES (?,?,?,?,NULL,?,?,?,?,?)
             ON CONFLICT(feed_id,guid) DO UPDATE SET url=excluded.url,title=excluded.title,author=excluded.author,content=excluded.content,published_at=excluded.published_at,discovered_at=excluded.discovered_at,updated_at=excluded.updated_at RETURNING id",
        )
        .bind(feed_id)
        .bind(guid)
        .bind(&link)
        .bind(&title)
        .bind(&author)
        .bind(&content)
        .bind(date)
        .bind(discovered)
        .bind(modified)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM entries_fts WHERE rowid=?")
            .bind(entry_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO entries_fts(rowid,title,author,summary,content) VALUES (?,?,?,?,?)",
        )
        .bind(entry_id)
        .bind(&title)
        .bind(author.as_deref().unwrap_or(""))
        .bind("")
        .bind(content.as_deref().unwrap_or(""))
        .execute(&mut *tx)
        .await?;
        let read = row.get::<bool, _>("is_read");
        let starred = row.get::<bool, _>("is_favorite");
        sqlx::query("INSERT INTO entry_state(entry_id,read,starred,read_at,updated_at) VALUES (?,?,?,?,?) ON CONFLICT(entry_id) DO UPDATE SET read=excluded.read,starred=excluded.starred,read_at=excluded.read_at,updated_at=excluded.updated_at")
            .bind(entry_id)
            .bind(read)
            .bind(starred)
            .bind(if read { last_user_modified.or(last_modified) } else { None })
            .bind(modified)
            .execute(&mut *tx)
            .await?;
        let mut labels = HashSet::new();
        if let Some(v) = row.get::<Option<String>, _>("labels") {
            labels.extend(
                v.split('\u{1f}')
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty()),
            );
        }
        if let Some(v) = row.get::<Option<String>, _>("tags") {
            labels.extend(
                v.split('#')
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty()),
            );
        }
        for label in labels {
            sqlx::query("INSERT OR IGNORE INTO labels(name) VALUES (?)")
                .bind(&label)
                .execute(&mut *tx)
                .await?;
            sqlx::query("INSERT OR IGNORE INTO entry_labels(entry_id,label_id) SELECT ?,id FROM labels WHERE name=?").bind(entry_id).bind(&label).execute(&mut *tx).await?;
            report.labels += 1;
        }
        report.entries += 1;
        pending += 1;
        if pending >= 1_000 {
            tx.commit().await?;
            tx = state.pool.begin().await?;
            pending = 0;
        }
    }
    tx.commit().await?;
    source_pool.close().await;
    Ok(report)
}

fn freshrss_database(path: &Path) -> Result<PathBuf, sqlx::Error> {
    let invalid = || {
        sqlx::Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "path must be a FreshRSS SQLite database, a user directory, or a data directory containing users/*/db.sqlite",
        ))
    };
    if path.is_file() {
        return Ok(path.to_path_buf());
    }
    if !path.is_dir() {
        return Err(invalid());
    }
    let direct = path.join("db.sqlite");
    if direct.is_file() {
        return Ok(direct);
    }
    let users = if path.ends_with("users") {
        path.to_path_buf()
    } else {
        path.join("data").join("users")
    };
    let users = if users.is_dir() {
        users
    } else {
        path.join("users")
    };
    if !users.is_dir() {
        return Err(invalid());
    }
    let candidates = std::fs::read_dir(users)
        .map_err(sqlx::Error::Io)?
        .filter_map(|entry| {
            let db = entry.ok()?.path().join("db.sqlite");
            db.is_file().then_some(db)
        })
        .collect::<Vec<_>>();
    match candidates.len() {
        1 => Ok(candidates[0].clone()),
        0 => Err(invalid()),
        _ => Err(sqlx::Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "multiple FreshRSS users found; pass one database path directly: {}",
                candidates
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ))),
    }
}

fn decode_content(bytes: &[u8]) -> Option<String> {
    if let Ok(v) = std::str::from_utf8(bytes) {
        return Some(v.to_string());
    }
    let candidates: &[&[u8]] = if bytes.len() > 4 {
        &[bytes, &bytes[4..]]
    } else {
        &[bytes]
    };
    for candidate in candidates {
        let mut out = String::new();
        if flate2::read::ZlibDecoder::new(*candidate)
            .read_to_string(&mut out)
            .is_ok()
        {
            return Some(out);
        }
        let mut out = String::new();
        if flate2::read::GzDecoder::new(*candidate)
            .read_to_string(&mut out)
            .is_ok()
        {
            return Some(out);
        }
    }
    None
}

fn nonempty(value: Option<String>, fallback: &str) -> String {
    value
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| fallback.into())
}
fn empty_to_none(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty())
}
fn timestamp(value: Option<i64>) -> Option<DateTime<Utc>> {
    value
        .filter(|v| *v > 0)
        .and_then(|v| DateTime::from_timestamp(v, 0))
}
fn http_url(value: &str) -> Option<String> {
    let url = url::Url::parse(value).ok()?;
    matches!(url.scheme(), "http" | "https").then(|| url.to_string())
}
async fn table_names(pool: &SqlitePool) -> Result<HashSet<String>, sqlx::Error> {
    Ok(
        sqlx::query_scalar::<_, String>("SELECT name FROM sqlite_master WHERE type='table'")
            .fetch_all(pool)
            .await?
            .into_iter()
            .collect(),
    )
}
fn freshrss_prefix(names: &HashSet<String>) -> Option<&str> {
    let mut feeds = names
        .iter()
        .filter(|name| name.as_str() == "feed" || name.ends_with("_feed"))
        .collect::<Vec<_>>();
    feeds.sort_by_key(|name| name.len());
    feeds.into_iter().find_map(|feed| {
        let prefix = feed.strip_suffix("feed")?;
        names.contains(&format!("{prefix}entry")).then_some(prefix)
    })
}
async fn columns(pool: &SqlitePool, table: &str) -> Result<HashSet<String>, sqlx::Error> {
    let quoted = table.replace('"', "");
    Ok(
        sqlx::query_scalar::<_, String>(&format!("SELECT name FROM pragma_table_info('{quoted}')"))
            .fetch_all(pool)
            .await?
            .into_iter()
            .collect(),
    )
}
