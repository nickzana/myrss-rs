use argon2::{password_hash::SaltString, Argon2, PasswordHasher};
use clap::{Parser, Subcommand};
use md5::Md5;
use sha2::Digest;
use std::{net::SocketAddr, sync::Arc};

#[derive(Parser)]
#[command(about = "myrss single-user RSS reader")]
struct Cli {
    #[arg(long, default_value = "config.toml")]
    config: String,
    #[command(subcommand)]
    command: Option<Command>,
}
#[derive(Subcommand)]
enum Command {
    Serve,
    CheckConfig,
    Backup { path: String },
    ImportFreshrss { path: String },
    CreateCredential { name: String },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let config = myrss::config::Config::load(std::path::Path::new(&cli.config))
        .map_err(|e| format!("configuration error: {e}"))?;
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| myrss::log_filter(&config));
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(&filter))
        .init();
    tracing::debug!(
        data_dir = %config.data_dir.display(),
        auth = ?config.auth.mode,
        feed_proxy = ?config.feed.proxy,
        "config loaded"
    );
    match cli.command.unwrap_or(Command::Serve) {
        Command::CheckConfig => {
            println!("configuration ok");
        }
        Command::Backup { path } => {
            let pool = myrss::db::connect(&config).await?;
            myrss::db::backup(&pool, std::path::Path::new(&path)).await?;
            println!("backup written to {path}");
        }
        Command::ImportFreshrss { path } => {
            let pool = myrss::db::connect(&config).await?;
            let state = myrss::AppState::new(config.clone(), pool);
            let report =
                myrss::migrate::import_freshrss(&state, std::path::Path::new(&path)).await?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Command::CreateCredential { name } => {
            let pool = myrss::db::connect(&config).await?;
            let username = format!("app-{}", myrss::db::random_token(8).to_lowercase());
            let password = myrss::db::random_token(24);
            let hash = Argon2::default()
                .hash_password(
                    password.as_bytes(),
                    &SaltString::generate(&mut rand_core::OsRng),
                )
                .map_err(|e| format!("password hash failed: {e}"))?
                .to_string();
            let fever = format!("{:x}", Md5::digest(format!("{username}:{password}")));
            let fever_hash = Argon2::default()
                .hash_password(
                    fever.as_bytes(),
                    &SaltString::generate(&mut rand_core::OsRng),
                )
                .map_err(|e| format!("password hash failed: {e}"))?
                .to_string();
            sqlx::query("INSERT INTO app_credentials(name,username,password_hash,fever_hash) VALUES (?,?,?,?)")
                .bind(&name).bind(&username).bind(hash).bind(fever_hash).execute(&pool).await?;
            println!(
                "username={username}
password={password}"
            );
        }
        Command::Serve => {
            let pool = myrss::db::connect(&config).await?;
            let state = Arc::new(myrss::AppState::new(config.clone(), pool));
            myrss::ingest::spawn_scheduler(state.clone());
            let app = myrss::api::router(state);
            let listener = tokio::net::TcpListener::bind(config.listen).await?;
            tracing::info!("listening on {}", config.listen);
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await?;
        }
    }
    Ok(())
}
