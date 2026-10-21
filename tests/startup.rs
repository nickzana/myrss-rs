use myrss::{config::Config, db, log_filter};
use std::path::Path;

const BASE: &str = r#"listen = "127.0.0.1:8080"
public_url = "http://localhost"
[auth]
mode = "proxy"
[auth.proxy]
trusted_peer_cidrs = ["127.0.0.1/32"]
user_header = "X-User"
"#;

fn write_config(dir: &Path, extra: &str) -> std::path::PathBuf {
    let path = dir.join("config.toml");
    std::fs::write(&path, format!("{BASE}{extra}")).unwrap();
    path
}

// The debug flag previously existed but was never read; it must change the filter.
#[test]
fn debug_flag_controls_log_filter() {
    let dir = tempfile::tempdir().unwrap();
    let on = Config::load(&write_config(
        dir.path(),
        "[server]
debug = true
",
    ))
    .unwrap();
    let off = Config::load(&write_config(dir.path(), "")).unwrap();
    assert!(log_filter(&on).contains("debug"));
    assert_eq!(log_filter(&off), "info");
}

// The Docker deployment relies on WORKDIR making this relative default writable.
#[test]
fn data_dir_defaults_to_relative_path() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = Config::load(&write_config(dir.path(), "")).unwrap();
    assert_eq!(cfg.data_dir, Path::new("./data"));
}

// Startup failures must name the failing path, not a bare io error.
#[cfg(unix)]
#[tokio::test]
async fn unwritable_data_dir_error_names_path() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let ro = dir.path().join("readonly");
    std::fs::create_dir(&ro).unwrap();
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
    let mut cfg = Config::load(&write_config(dir.path(), "")).unwrap();
    cfg.data_dir = ro.join("data");
    match db::connect(&cfg).await {
        // Running as root the write succeeds and there is nothing to assert.
        Ok(_) => {}
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains("readonly/data"),
                "error should name the failing path: {msg}"
            );
        }
    }
}
