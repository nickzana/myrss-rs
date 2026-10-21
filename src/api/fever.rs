use crate::{api::native, db, AppState};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};
use sqlx::Row;
use std::sync::Arc;

pub async fn handle(
    State(s): State<Arc<AppState>>,
    Query(query): Query<std::collections::HashMap<String, String>>,
    body: String,
) -> Result<Json<Value>, StatusCode> {
    let mut params = url::form_urlencoded::parse(body.as_bytes())
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect::<std::collections::HashMap<_, _>>();
    params.extend(query);
    let key = params.get("api_key").ok_or(StatusCode::UNAUTHORIZED)?;
    let credential = native::verify_fever_key(&s, key).await?;
    let mut out = json!({"api_version":3,"auth":1});
    if params.contains_key("groups") {
        out["groups"] = groups(&s).await?;
    }
    if params.contains_key("feeds") {
        out["feeds"] = feeds(&s).await?;
        out["feeds_groups"] = feeds_groups(&s).await?;
    }
    if params.contains_key("favicons") {
        out["favicons"] = json!([]);
    }
    if params.contains_key("items") {
        out["items"] = items(&s, &params).await?;
        out["total_items"] = json!(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM entries")
            .fetch_one(&s.pool)
            .await
            .unwrap_or(0));
    }
    if params.contains_key("unread_item_ids") {
        out["unread_item_ids"] = id_list(&s, false).await?;
    }
    if params.contains_key("saved_item_ids") {
        out["saved_item_ids"] = id_list(&s, true).await?;
    }
    if let Some(mark) = params.get("mark") {
        apply_mark(&s, &params, mark).await?;
        match params.get("as").map(String::as_str) {
            Some("read") | Some("unread") => {
                out["unread_item_ids"] = id_list(&s, false).await?;
            }
            Some("saved") | Some("unsaved") => {
                out["saved_item_ids"] = id_list(&s, true).await?;
            }
            _ => {}
        }
    }
    let _ = credential;
    Ok(Json(out))
}

async fn groups(s: &AppState) -> Result<Value, StatusCode> {
    let rows =
        sqlx::query("SELECT DISTINCT folder FROM feeds WHERE folder IS NOT NULL ORDER BY folder")
            .fetch_all(&s.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Value::Array(
        rows.into_iter()
            .enumerate()
            .map(|(i, r)| {
                let f: String = r.get(0);
                json!({"id":i+1,"title":f})
            })
            .collect(),
    ))
}
async fn feeds(s: &AppState) -> Result<Value, StatusCode> {
    let rows = sqlx::query("SELECT id,title,url,site_url,last_error FROM feeds ORDER BY id")
        .fetch_all(&s.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Value::Array(rows.into_iter().map(|r|json!({"id":r.get::<i64,_>("id"),"favicon_id":0,"title":r.get::<String,_>("title"),"url":r.get::<String,_>("url"),"site_url":r.get::<Option<String>,_>("site_url"),"is_spark":0,"last_updated_on_time":0,"error":r.get::<Option<String>,_>("last_error")})).collect()))
}
async fn feeds_groups(s: &AppState) -> Result<Value, StatusCode> {
    let rows = sqlx::query("SELECT id,folder FROM feeds WHERE folder IS NOT NULL")
        .fetch_all(&s.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let folders = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT folder FROM feeds WHERE folder IS NOT NULL ORDER BY folder",
    )
    .fetch_all(&s.pool)
    .await
    .unwrap_or_default();
    Ok(Value::Array(
        rows.into_iter()
            .map(|r| {
                let folder: String = r.get("folder");
                let gid = folders.iter().position(|f| f == &folder).unwrap() + 1;
                json!({"group_id":gid,"feed_ids":r.get::<i64,_>("id").to_string()})
            })
            .collect(),
    ))
}
async fn items(
    s: &AppState,
    p: &std::collections::HashMap<String, String>,
) -> Result<Value, StatusCode> {
    let mut where_sql = "1=1".to_string();
    let mut binds = vec![];
    if let Some(v) = p.get("since_id").and_then(|v| v.parse::<i64>().ok()) {
        where_sql.push_str(" AND e.id>?");
        binds.push(v.to_string())
    }
    if let Some(v) = p.get("max_id").and_then(|v| v.parse::<i64>().ok()) {
        where_sql.push_str(" AND e.id<?");
        binds.push(v.to_string())
    }
    if let Some(v) = p.get("with_ids") {
        let ids = v
            .split(',')
            .filter_map(|x| x.parse::<i64>().ok())
            .map(|x| x.to_string())
            .collect::<Vec<_>>();
        if !ids.is_empty() {
            where_sql.push_str(&format!(" AND e.id IN ({})", ids.join(",")))
        }
    }
    let sql=format!("SELECT e.id,e.feed_id,e.title,e.author,e.url,e.summary,e.content,e.published_at,st.read,st.starred FROM entries e JOIN entry_state st ON st.entry_id=e.id WHERE {} ORDER BY e.id ASC LIMIT 1000",where_sql);
    let mut q = sqlx::query(&sql);
    for b in binds {
        q = q.bind(b)
    }
    let rows = q
        .fetch_all(&s.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Value::Array(rows.into_iter().map(|r|{let ts:Option<chrono::DateTime<chrono::Utc>>=r.get("published_at");json!({"id":r.get::<i64,_>("id"),"feed_id":r.get::<i64,_>("feed_id"),"title":r.get::<String,_>("title"),"author":r.get::<Option<String>,_>("author"),"html":r.get::<Option<String>,_>("content").or_else(||r.get("summary")),"url":r.get::<Option<String>,_>("url"),"is_saved":r.get::<bool,_>("starred") as i64,"is_read":r.get::<bool,_>("read") as i64,"created_on_time":ts.map(|t|t.timestamp()).unwrap_or(0)})}).collect()))
}
async fn id_list(s: &AppState, saved: bool) -> Result<Value, StatusCode> {
    let ids = sqlx::query_scalar::<_, i64>(if saved {
        "SELECT entry_id FROM entry_state WHERE starred=1 ORDER BY entry_id"
    } else {
        "SELECT entry_id FROM entry_state WHERE read=0 ORDER BY entry_id"
    })
    .fetch_all(&s.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(json!(ids
        .into_iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(",")))
}
async fn apply_mark(
    s: &AppState,
    p: &std::collections::HashMap<String, String>,
    mark: &str,
) -> Result<(), StatusCode> {
    let id = p.get("id").and_then(|v| v.parse::<i64>().ok());
    let as_value = p.get("as").map(String::as_str);
    match mark {
        "item" => {
            if let Some(id) = id {
                let state = crate::models::EntryState {
                    read: match as_value {
                        Some("read") => Some(true),
                        Some("unread") => Some(false),
                        _ => None,
                    },
                    starred: match as_value {
                        Some("saved") => Some(true),
                        Some("unsaved") => Some(false),
                        _ => None,
                    },
                    labels: None,
                };
                db::set_entry_state(&s.pool, id, &state)
                    .await
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            }
        }
        "feed" => {
            if as_value == Some("read") {
                db::mark_all_read(&s.pool, id)
                    .await
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            }
        }
        "group" if as_value == Some("read") => {
            let group_id = id.unwrap_or_default();
            if group_id == 0 {
                db::mark_all_read(&s.pool, None)
                    .await
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            } else {
                let folders = sqlx::query_scalar::<_, String>(
                    "SELECT DISTINCT folder FROM feeds WHERE folder IS NOT NULL ORDER BY folder",
                )
                .fetch_all(&s.pool)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                let folder = folders
                    .get(
                        usize::try_from(group_id)
                            .unwrap_or(usize::MAX)
                            .saturating_sub(1),
                    )
                    .ok_or(StatusCode::BAD_REQUEST)?;
                db::mark_label_read(&s.pool, folder)
                    .await
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            }
        }
        _ => {}
    }
    Ok(())
}
