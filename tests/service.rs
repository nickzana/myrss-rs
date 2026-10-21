use chrono::{Duration, Utc};
use myrss::{
    config::{AuthConfig, AuthMode, Config, FeedConfig, LocalAuth, ServerConfig},
    db::{self, EntryQuery},
};
use std::net::SocketAddr;

fn config(dir: &std::path::Path) -> Config {
    let password = dir.join("password");
    std::fs::write(&password, "0123456789abcdef").unwrap();
    Config {
        listen: "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
        public_url: "http://localhost".into(),
        data_dir: dir.to_path_buf(),
        auth: AuthConfig {
            mode: AuthMode::Local,
            local: Some(LocalAuth {
                password_file: password,
            }),
            oidc: None,
            proxy: None,
            session_idle_hours: 12,
        },
        feed: FeedConfig::default(),
        server: ServerConfig::default(),
    }
}

#[tokio::test]
async fn sqlite_lifecycle_state_search_and_backup() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let pool = db::connect(&cfg).await.unwrap();
    let feed = db::add_feed(&pool, "Feed", "https://example.com/rss", Some("Tech"))
        .await
        .unwrap();
    let id = db::upsert_entry(
        &pool,
        feed,
        "guid",
        Some("https://example.com/a"),
        "Title",
        Some("Ada"),
        Some("summary"),
        Some("<p>content</p>"),
        Some(Utc::now()),
    )
    .await
    .unwrap();
    let same = db::upsert_entry(
        &pool,
        feed,
        "guid",
        Some("https://example.com/a"),
        "Title 2",
        Some("Ada"),
        Some("summary"),
        Some("<p>new</p>"),
        Some(Utc::now()),
    )
    .await
    .unwrap();
    assert_eq!(id, same);
    db::set_entry_state(
        &pool,
        id,
        &myrss::models::EntryState {
            read: Some(true),
            starred: Some(true),
            labels: Some(vec!["Keep".into()]),
        },
    )
    .await
    .unwrap();
    let rows = db::list_entries(
        &pool,
        &EntryQuery {
            filter: Some("starred".into()),
            feed_id: None,
            folder: None,
            label: None,
            q: Some("Title".into()),
            before: None,
            limit: None,
            unread: None,
            starred: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(rows.len(), 1);
    let backup = dir.path().join("backup.db");
    db::backup(&pool, &backup).await.unwrap();
    assert!(backup.exists());
}

#[tokio::test]
async fn retention_keeps_identity_and_protected_entries() {
    let dir = tempfile::tempdir().unwrap();
    let pool = db::connect(&config(dir.path())).await.unwrap();
    let feed = db::add_feed(&pool, "Feed", "https://example.com/rss", None)
        .await
        .unwrap();
    let old = Utc::now() - Duration::days(120);
    let expired = db::upsert_entry(&pool, feed, "old", None, "Old", None, None, None, Some(old))
        .await
        .unwrap();
    db::set_entry_state(
        &pool,
        expired,
        &myrss::models::EntryState {
            read: Some(true),
            starred: None,
            labels: None,
        },
    )
    .await
    .unwrap();
    let kept = db::upsert_entry(
        &pool,
        feed,
        "keep",
        None,
        "Keep",
        None,
        None,
        None,
        Some(old),
    )
    .await
    .unwrap();
    db::set_entry_state(
        &pool,
        kept,
        &myrss::models::EntryState {
            read: Some(true),
            starred: Some(true),
            labels: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(db::retain(&pool, 90).await.unwrap(), 1);
    let replay = db::upsert_entry(
        &pool,
        feed,
        "old",
        None,
        "Old again",
        None,
        None,
        None,
        Some(old),
    )
    .await
    .unwrap();
    assert_eq!(replay, 0);
    let rows = db::list_entries(
        &pool,
        &EntryQuery {
            filter: None,
            feed_id: None,
            folder: None,
            label: None,
            q: None,
            before: None,
            limit: None,
            unread: None,
            starred: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "Keep");
}

#[test]
fn opml_round_trip() {
    let feeds = vec![(
        "A".to_string(),
        "https://a/rss".to_string(),
        Some("Tech".to_string()),
    )];
    let xml = myrss::ingest::opml::export_opml(&feeds);
    let parsed = myrss::ingest::opml::parse_opml(&xml).unwrap();
    assert_eq!(parsed, feeds);
}

#[tokio::test]
async fn freshrss_import_preserves_feeds_articles_state_and_labels() {
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("freshrss.db");
    let source = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&source_path)
            .create_if_missing(true),
    )
    .await
    .unwrap();
    sqlx::query(
        r#"CREATE TABLE `_category`(id INTEGER PRIMARY KEY,name TEXT NOT NULL);
        CREATE TABLE `_feed`(id INTEGER PRIMARY KEY,url TEXT NOT NULL,category INTEGER,name TEXT NOT NULL,website TEXT,httpAuth TEXT);
        CREATE TABLE `_entry`(id INTEGER PRIMARY KEY,guid TEXT,title TEXT,author TEXT,content BLOB,link TEXT,date INTEGER,lastSeen INTEGER,lastModified INTEGER,lastUserModified INTEGER,is_read INTEGER,is_favorite INTEGER,id_feed INTEGER,tags TEXT);
        CREATE TABLE `_tag`(id INTEGER PRIMARY KEY,name TEXT NOT NULL);
        CREATE TABLE `_entrytag`(id_tag INTEGER NOT NULL,id_entry INTEGER NOT NULL);"#,
    )
    .execute(&source)
    .await
    .unwrap();
    sqlx::query("INSERT INTO `_category` VALUES (1,'Tech');INSERT INTO `_feed` VALUES (7,'https://example.com/rss',1,'Example','https://example.com','user:pass');INSERT INTO `_entry` VALUES (42,'g1','Migrated','Ada',CAST('<p>Saved <script>x</script></p>' AS BLOB),'https://example.com/a',1700000000,1700000100,1700000200,1700000300,1,1,7,' #inline');INSERT INTO `_tag` VALUES (5,'Keep');INSERT INTO `_entrytag` VALUES (5,42)").execute(&source).await.unwrap();
    source.close().await;

    let target_dir = dir.path().join("target");
    std::fs::create_dir_all(&target_dir).unwrap();
    let cfg = config(&target_dir);
    let pool = db::connect(&cfg).await.unwrap();
    let state = myrss::AppState::new(cfg, pool.clone());
    let report = myrss::migrate::import_freshrss(&state, &source_path)
        .await
        .unwrap();
    assert_eq!(report.feeds, 1);
    assert_eq!(report.entries, 1);
    assert!(report.labels >= 2);
    let feed = db::get_feed_by_url(&pool, "https://example.com/rss")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(feed.folder.as_deref(), Some("Tech"));
    assert_eq!(feed.site_url.as_deref(), Some("https://example.com/"));
    assert_eq!(feed.http_auth.as_deref(), Some("user:pass"));
    let rows = db::list_entries(
        &pool,
        &EntryQuery {
            filter: Some("starred".into()),
            feed_id: Some(feed.id),
            folder: None,
            label: Some("Keep".into()),
            q: Some("Saved".into()),
            before: None,
            limit: None,
            unread: None,
            starred: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].read);
    assert!(rows[0].starred);
    let detail = db::get_entry(&pool, rows[0].id).await.unwrap().unwrap();
    assert_eq!(
        detail.labels,
        vec!["Keep".to_string(), "inline".to_string()]
    );
    assert!(!detail.content.unwrap().contains("script"));
}

#[tokio::test]
async fn freshrss_import_accepts_custom_prefix_and_data_directory() {
    let dir = tempfile::tempdir().unwrap();
    let user_dir = dir.path().join("freshrss").join("users").join("alice");
    std::fs::create_dir_all(&user_dir).unwrap();
    let source_path = user_dir.join("db.sqlite");
    let source = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&source_path)
            .create_if_missing(true),
    )
    .await
    .unwrap();
    sqlx::query(
        r#"CREATE TABLE `custom_feed`(id INTEGER PRIMARY KEY,url TEXT NOT NULL,name TEXT NOT NULL);
        CREATE TABLE `custom_entry`(id INTEGER PRIMARY KEY,guid TEXT,title TEXT,author TEXT,content BLOB,link TEXT,date INTEGER,lastSeen INTEGER,is_read INTEGER,is_favorite INTEGER,id_feed INTEGER,tags TEXT);
        INSERT INTO `custom_feed` VALUES (1,'https://example.com/rss','Prefixed');
        INSERT INTO `custom_entry` VALUES (1,'g','Article',NULL,CAST('<p>x</p>' AS BLOB),'https://example.com/a',1700000000,1700000100,0,0,1,NULL);"#,
    )
    .execute(&source)
    .await
    .unwrap();
    source.close().await;

    let target_dir = dir.path().join("target");
    std::fs::create_dir_all(&target_dir).unwrap();
    let cfg = config(&target_dir);
    let pool = db::connect(&cfg).await.unwrap();
    let state = myrss::AppState::new(cfg, pool.clone());
    let report = myrss::migrate::import_freshrss(&state, &dir.path().join("freshrss"))
        .await
        .unwrap();
    assert_eq!((report.feeds, report.entries), (1, 1));
    assert!(db::get_feed_by_url(&pool, "https://example.com/rss")
        .await
        .unwrap()
        .is_some());
}
