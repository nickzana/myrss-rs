use crate::{api::native, db, AppState};
use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use sqlx::Row;
use std::{collections::HashMap, sync::Arc};

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/accounts/ClientLogin", post(client_login))
        .route("/reader/api/0/token", get(token))
        .route("/reader/api/0/user-info", get(user_info))
        .route("/reader/api/0/subscription/list", get(subscription_list))
        .route("/reader/api/0/tag/list", get(tag_list))
        .route("/reader/api/0/unread-count", get(unread_count))
        .route("/reader/api/0/stream/items/ids", get(item_ids))
        .route("/reader/api/0/stream/items/contents", post(item_contents))
        .route(
            "/reader/api/0/stream/contents/{*stream}",
            get(stream_contents),
        )
        .route("/reader/api/0/edit-tag", post(edit_tag))
        .route("/reader/api/0/mark-all-as-read", post(mark_all_as_read))
        .route("/reader/api/0/subscription/edit", post(subscription_edit))
        .route("/reader/api/0/subscription/quickadd", post(quickadd))
}

type R<T> = Result<T, StatusCode>;
fn unauthorized() -> StatusCode {
    StatusCode::UNAUTHORIZED
}
pub async fn credential_token(state: &AppState, headers: &HeaderMap) -> R<i64> {
    let value = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(unauthorized)?;
    let token = value
        .strip_prefix("GoogleLogin auth=")
        .or_else(|| value.strip_prefix("Bearer "))
        .ok_or_else(unauthorized)?
        .split('/')
        .next_back()
        .unwrap_or_default();
    let row=sqlx::query("SELECT t.credential_id FROM api_tokens t JOIN app_credentials c ON c.id=t.credential_id WHERE t.token_hash=? AND t.expires_at>? AND c.enabled=1").bind(db::token_hash(token)).bind(Utc::now()).fetch_optional(&state.pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?.ok_or_else(unauthorized)?;
    Ok(row.get(0))
}

async fn client_login(State(s): State<Arc<AppState>>, body: String) -> R<Response> {
    let form = form_map(&body);
    let email = form.get("Email").ok_or(StatusCode::BAD_REQUEST)?;
    let passwd = form.get("Passwd").ok_or(StatusCode::BAD_REQUEST)?;
    let id = native::verify_app_password(&s, email, passwd).await?;
    let token = db::random_token(32);
    sqlx::query("INSERT INTO api_tokens(credential_id,token_hash,expires_at) VALUES (?,?,?)")
        .bind(id)
        .bind(db::token_hash(&token))
        .bind(Utc::now() + chrono::Duration::days(30))
        .execute(&s.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let auth = format!("{email}/{token}");
    Ok((
        [(header::CONTENT_TYPE, "text/plain")],
        format!("SID={auth}\nLSID={auth}\nAuth={auth}\n"),
    )
        .into_response())
}
async fn token(State(s): State<Arc<AppState>>, h: HeaderMap) -> R<String> {
    credential_token(&s, &h).await?;
    Ok(db::random_token(24))
}
async fn user_info(State(s): State<Arc<AppState>>, h: HeaderMap) -> R<Json<serde_json::Value>> {
    credential_token(&s, &h).await?;
    Ok(Json(
        json!({"userId":"1","userName":"owner","userProfileId":"1","userEmail":"owner@localhost","isBloggerUser":false,"signupTimeSec":0,"isMultiLoginEnabled":false}),
    ))
}
async fn subscription_list(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
) -> R<Json<serde_json::Value>> {
    credential_token(&s, &h).await?;
    let feeds = sqlx::query("SELECT id,title,url,site_url,folder FROM feeds ORDER BY title")
        .fetch_all(&s.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let arr=feeds.into_iter().map(|r|{let id:i64=r.get("id");let url:String=r.get("url");let folder:Option<String>=r.get("folder");json!({"id":format!("feed/{id}"),"title":r.get::<String,_>("title"),"categories":folder.map(|f|vec![json!({"id":format!("user/-/label/{f}"),"label":f})]).unwrap_or_default(),"sortid":format!("{:08x}",id),"firstitemmsec":"0","url":url,"htmlUrl":r.get::<Option<String>,_>("site_url"),"iconUrl":""})}).collect::<Vec<_>>();
    Ok(Json(json!({"subscriptions":arr})))
}
async fn tag_list(State(s): State<Arc<AppState>>, h: HeaderMap) -> R<Json<serde_json::Value>> {
    credential_token(&s, &h).await?;
    let mut tags =
        vec![json!({"id":"user/-/state/com.google/starred","sortid":"1","type":"folder"})];
    for f in sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT folder FROM feeds WHERE folder IS NOT NULL",
    )
    .fetch_all(&s.pool)
    .await
    .unwrap_or_default()
    {
        tags.push(json!({"id":format!("user/-/label/{f}"),"label":f,"sortid":"2","type":"folder"}));
    }
    for l in sqlx::query_scalar::<_, String>("SELECT name FROM labels")
        .fetch_all(&s.pool)
        .await
        .unwrap_or_default()
    {
        tags.push(json!({"id":format!("user/-/label/{l}"),"label":l,"sortid":"3","type":"tag"}));
    }
    Ok(Json(json!({"tags":tags})))
}
async fn unread_count(State(s): State<Arc<AppState>>, h: HeaderMap) -> R<Json<serde_json::Value>> {
    credential_token(&s, &h).await?;
    let rows=sqlx::query("SELECT f.id,COALESCE(SUM(CASE WHEN st.read=0 THEN 1 ELSE 0 END),0) n FROM feeds f LEFT JOIN entries e ON e.feed_id=f.id LEFT JOIN entry_state st ON st.entry_id=e.id GROUP BY f.id").fetch_all(&s.pool).await.unwrap_or_default();
    let counts=rows.into_iter().map(|r|json!({"id":format!("feed/{}",r.get::<i64,_>("id")),"count":r.get::<i64,_>("n"),"newestItemTimestampUsec":0})).collect::<Vec<_>>();
    Ok(Json(
        json!({"max":counts.iter().map(|v|v["count"].as_i64().unwrap()).max().unwrap_or(0),"unreadcounts":counts}),
    ))
}

#[derive(Deserialize)]
struct StreamQ {
    s: Option<String>,
    n: Option<i64>,
    c: Option<String>,
    r: Option<String>,
    ot: Option<i64>,
    xt: Option<String>,
    it: Option<String>,
    output: Option<String>,
}
async fn item_ids(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Query(q): Query<StreamQ>,
) -> R<Json<serde_json::Value>> {
    credential_token(&s, &h).await?;
    let (ids, next) = query_ids(&s, &q).await?;
    Ok(Json(
        json!({"itemRefs":ids.iter().map(|(id,ts)|json!({"id":id.to_string(),"directStreamIds":[],"timestampUsec":ts*1_000_000})).collect::<Vec<_>>(),"continuation":next}),
    ))
}
async fn stream_contents(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    axum::extract::Path(stream): axum::extract::Path<String>,
    Query(q): Query<StreamQ>,
) -> R<Json<serde_json::Value>> {
    credential_token(&s, &h).await?;
    let mut q = q;
    q.s = Some(stream);
    let (ids, _) = query_ids(&s, &q).await?;
    let items = items_for(&s, ids.into_iter().map(|v| v.0).collect()).await?;
    Ok(Json(
        json!({"direction":"ltr","id":"","title":"myrss","items":items}),
    ))
}
async fn item_contents(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    body: String,
) -> R<Json<serde_json::Value>> {
    credential_token(&s, &h).await?;
    let pairs = form_pairs(&body);
    let ids = pairs
        .iter()
        .filter(|(k, _)| k == "i")
        .filter_map(|(_, v)| parse_item_id(v))
        .take(250)
        .collect();
    Ok(Json(json!({"items":items_for(&s,ids).await?})))
}

async fn query_ids(s: &AppState, q: &StreamQ) -> R<(Vec<(i64, i64)>, Option<String>)> {
    let _ = (&q.r, &q.it, &q.output);
    let limit = q.n.unwrap_or(1000).clamp(1, 5000);
    let mut where_sql = vec!["1=1".to_string()];
    let mut bind = Vec::<String>::new();
    if let Some(stream) = &q.s {
        let decoded = stream.clone();
        if let Some(feed_id) = decoded.strip_prefix("feed/") {
            where_sql.push("e.feed_id=?".into());
            bind.push(
                feed_id
                    .parse::<i64>()
                    .map_err(|_| StatusCode::BAD_REQUEST)?
                    .to_string(),
            );
        } else if let Some(label) = decoded.strip_prefix("user/-/label/") {
            where_sql.push("(f.folder=? OR l.name=?)".into());
            bind.push(label.to_string());
            bind.push(label.to_string());
        } else if decoded == "user/-/state/com.google/starred" {
            where_sql.push("st.starred=1".into());
        } else if decoded == "user/-/state/com.google/read" {
            where_sql.push("st.read=1".into());
        } else if decoded == "user/-/state/com.google/unread" {
            where_sql.push("st.read=0".into());
        }
    }
    if q.xt.as_deref() == Some("user/-/state/com.google/read") {
        where_sql.push("st.read=0".into())
    }
    if let Some(ot) = q.ot {
        where_sql.push("CAST(strftime('%s',e.discovered_at) AS INTEGER)>=?".into());
        bind.push(ot.to_string())
    }
    if let Some(c) = &q.c {
        if let Ok(id) = c.parse::<i64>() {
            where_sql.push("e.id<?".into());
            bind.push(id.to_string())
        }
    }
    let direction = if q.r.as_deref() == Some("o") {
        "ASC"
    } else {
        "DESC"
    };
    let sql=format!("SELECT DISTINCT e.id,COALESCE(CAST(strftime('%s',e.published_at) AS INTEGER),CAST(strftime('%s',e.discovered_at) AS INTEGER)) ts FROM entries e JOIN feeds f ON f.id=e.feed_id JOIN entry_state st ON st.entry_id=e.id LEFT JOIN entry_labels el ON el.entry_id=e.id LEFT JOIN labels l ON l.id=el.label_id WHERE {} ORDER BY ts {direction},e.id {direction} LIMIT ?",where_sql.join(" AND "));
    let mut query = sqlx::query(&sql);
    for b in bind {
        query = query.bind(b)
    }
    query = query.bind(limit + 1);
    let rows = query
        .fetch_all(&s.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let next = if rows.len() > limit as usize {
        Some(rows[limit as usize - 1].get::<i64, _>(0).to_string())
    } else {
        None
    };
    let ids = rows
        .into_iter()
        .take(limit as usize)
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    Ok((ids, next))
}
async fn items_for(s: &AppState, ids: Vec<i64>) -> R<Vec<serde_json::Value>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let mut items = vec![];
    for id in ids {
        if let Some(e) = db::get_entry(&s.pool, id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        {
            let mut cats = vec!["user/-/state/com.google/reading-list".to_string()];
            if e.item.read {
                cats.push("user/-/state/com.google/read".into())
            }
            if e.item.starred {
                cats.push("user/-/state/com.google/starred".into())
            }
            if let Some(f) = &e.item.folder {
                cats.push(format!("user/-/label/{f}"))
            }
            cats.extend(e.labels.iter().map(|l| format!("user/-/label/{l}")));
            items.push(json!({"crawlTimeMsec":e.item.discovered_at.timestamp_millis().to_string(),"timestampUsec":e.item.discovered_at.timestamp_micros().to_string(),"id":e.item.id.to_string(),"categories":cats,"title":e.item.title,"published":e.item.published_at.map(|d|d.timestamp()).unwrap_or(0),"updated":e.item.published_at.map(|d|d.timestamp()).unwrap_or(0),"canonical":[{"href":e.item.url}],"alternate":[{"href":e.item.url}],"summary":{"direction":"ltr","content":e.summary.or(e.content).unwrap_or_default()},"author":e.item.author,"origin":{"streamId":format!("feed/{}",e.item.feed_id),"title":e.item.feed_title,"htmlUrl":e.item.url}}));
        }
    }
    Ok(items)
}

async fn edit_tag(State(s): State<Arc<AppState>>, h: HeaderMap, body: String) -> R<&'static str> {
    credential_token(&s, &h).await?;
    let pairs = form_pairs(&body);
    let adds = pairs
        .iter()
        .filter(|(k, _)| k == "a")
        .map(|(_, v)| v.clone())
        .collect::<Vec<_>>();
    let removes = pairs
        .iter()
        .filter(|(k, _)| k == "r")
        .map(|(_, v)| v.clone())
        .collect::<Vec<_>>();
    for (_, idstr) in pairs.iter().filter(|(k, _)| k == "i") {
        let Some(id) = parse_item_id(idstr) else {
            continue;
        };
        for tag in &adds {
            match tag.as_str() {
                "user/-/state/com.google/read" => db::set_entry_state(
                    &s.pool,
                    id,
                    &crate::models::EntryState {
                        read: Some(true),
                        starred: None,
                        labels: None,
                    },
                )
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                "user/-/state/com.google/starred" => db::set_entry_state(
                    &s.pool,
                    id,
                    &crate::models::EntryState {
                        read: None,
                        starred: Some(true),
                        labels: None,
                    },
                )
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                _ => {
                    if let Some(label) = tag.strip_prefix("user/-/label/") {
                        db::add_entry_label(&s.pool, id, label)
                            .await
                            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                    }
                }
            }
        }
        for tag in &removes {
            match tag.as_str() {
                "user/-/state/com.google/read" => db::set_entry_state(
                    &s.pool,
                    id,
                    &crate::models::EntryState {
                        read: Some(false),
                        starred: None,
                        labels: None,
                    },
                )
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                "user/-/state/com.google/starred" => db::set_entry_state(
                    &s.pool,
                    id,
                    &crate::models::EntryState {
                        read: None,
                        starred: Some(false),
                        labels: None,
                    },
                )
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                _ => {
                    if let Some(label) = tag.strip_prefix("user/-/label/") {
                        db::remove_entry_label(&s.pool, id, label)
                            .await
                            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                    }
                }
            }
        }
    }
    Ok("OK")
}

async fn mark_all_as_read(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    body: String,
) -> R<&'static str> {
    credential_token(&s, &h).await?;
    let form = form_map(&body);
    let stream = form.get("s").map(String::as_str).unwrap_or_default();
    if let Some(feed_id) = stream
        .strip_prefix("feed/")
        .and_then(|v| v.parse::<i64>().ok())
    {
        db::mark_all_read(&s.pool, Some(feed_id))
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    } else if let Some(label) = stream.strip_prefix("user/-/label/") {
        db::mark_label_read(&s.pool, label)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    } else if matches!(
        stream,
        "user/-/state/com.google/reading-list" | "user/-/state/com.google/unread"
    ) {
        db::mark_all_read(&s.pool, None)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    } else {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok("OK")
}

async fn subscription_edit(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    body: String,
) -> R<&'static str> {
    credential_token(&s, &h).await?;
    let f = form_map(&body);
    let stream = f
        .get("s")
        .and_then(|v| v.strip_prefix("feed/"))
        .and_then(|v| v.parse::<i64>().ok());
    match f.get("ac").map(String::as_str) {
        Some("subscribe") => {
            let url = f
                .get("s")
                .and_then(|v| v.strip_prefix("feed/"))
                .ok_or(StatusCode::BAD_REQUEST)?;
            let folder = f.get("a").and_then(|v| v.strip_prefix("user/-/label/"));
            s.fetcher
                .discover_and_add(&s.pool, url, folder)
                .await
                .map_err(|_| StatusCode::BAD_GATEWAY)?;
        }
        Some("unsubscribe") => {
            if let Some(id) = stream {
                sqlx::query("DELETE FROM feeds WHERE id=?")
                    .bind(id)
                    .execute(&s.pool)
                    .await
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            }
        }
        Some("edit") => {
            if let Some(id) = stream {
                let folder = f
                    .get("a")
                    .and_then(|v| v.strip_prefix("user/-/label/"))
                    .or_else(|| f.get("a").map(String::as_str));
                let has_folder_change = f.contains_key("a") || f.contains_key("r");
                sqlx::query(
                    "UPDATE feeds SET title=COALESCE(?,title),folder=CASE WHEN ? THEN ? ELSE folder END WHERE id=?",
                )
                    .bind(f.get("t"))
                    .bind(has_folder_change)
                    .bind(folder)
                    .bind(id)
                    .execute(&s.pool)
                    .await
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            }
        }
        _ => {
            if let Some(u) = f.get("quickadd") {
                s.fetcher
                    .discover_and_add(&s.pool, u, None)
                    .await
                    .map_err(|_| StatusCode::BAD_GATEWAY)?;
            }
        }
    }
    Ok("OK")
}

async fn quickadd(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    body: String,
) -> R<Json<serde_json::Value>> {
    credential_token(&s, &h).await?;
    let f = form_map(&body);
    let url = f
        .get("quickadd")
        .or_else(|| f.get("url"))
        .ok_or(StatusCode::BAD_REQUEST)?;
    let id = s
        .fetcher
        .discover_and_add(&s.pool, url, None)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(Json(
        json!({"query":url,"numResults":1,"streamId":format!("feed/{id}")}),
    ))
}

fn form_map(body: &str) -> HashMap<String, String> {
    form_pairs(body).into_iter().collect()
}
fn form_pairs(body: &str) -> Vec<(String, String)> {
    url::form_urlencoded::parse(body.as_bytes())
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}
fn parse_item_id(v: &str) -> Option<i64> {
    let prefixed = v.starts_with("tag:google.com,2005:reader/item/");
    let candidate = v
        .strip_prefix("tag:google.com,2005:reader/item/")
        .unwrap_or_else(|| v.rsplit('/').next().unwrap_or(v));
    if prefixed || (candidate.len() > 1 && candidate.starts_with('0')) {
        i64::from_str_radix(candidate, 16).ok()
    } else {
        candidate.parse().ok()
    }
}
