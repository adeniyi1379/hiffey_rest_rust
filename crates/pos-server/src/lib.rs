//! Hiffey POS LAN server: axum + SQLite, hosted by Terminal A.

#![forbid(unsafe_code)]

pub mod audit;
pub mod auth;
pub mod catalog;
pub mod db;
pub mod error;
pub mod events;
pub mod receipt;
pub mod routes;
pub mod sales;

use std::{net::SocketAddr, path::PathBuf, time::Duration};

use axum::Router;
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::events::Events;

/// Shared state handed to every request.
#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub events: Events,
    pub version: String,
    pub started_at: DateTime<Utc>,
}

impl AppState {
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            events: Events::new(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            started_at: Utc::now(),
        }
    }
}

/// Builds the application router with its state attached.
pub fn router(state: AppState) -> Router {
    routes::router().with_state(state)
}

/// Everything the server needs to start.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Address to listen on, e.g. `0.0.0.0:47800` for LAN access.
    pub bind_addr: SocketAddr,
    /// SQLite database file.
    pub db_path: PathBuf,
    /// Directory that receives a copy of the database on each start.
    pub backup_dir: PathBuf,
}

impl ServerConfig {
    #[must_use]
    pub fn new(
        bind_addr: SocketAddr,
        db_path: impl Into<PathBuf>,
        backup_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            bind_addr,
            db_path: db_path.into(),
            backup_dir: backup_dir.into(),
        }
    }
}

/// Runs the server until Ctrl+C: back up, migrate, then serve.
pub async fn run(config: ServerConfig) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    init_tracing();

    if let Some(parent) = config.db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if let Some(path) = db::backup_before_start(&config.db_path, &config.backup_dir)? {
        tracing::info!(backup = %path.display(), "database backed up before migration");
    }

    let pool = db::connect(&config.db_path).await?;
    db::migrate(&pool).await?;

    let listener = tokio::net::TcpListener::bind(config.bind_addr).await?;
    tracing::info!(address = %listener.local_addr()?, "Hiffey POS server listening");

    let app = router(AppState::new(pool));
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("server stopped");
    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,pos_server=debug"));
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .try_init();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }

    tracing::info!("shutdown requested");
}

/// Advisory duration used by the UI when it polls for server health.
pub const HEALTH_POLL_INTERVAL: Duration = Duration::from_secs(5);
