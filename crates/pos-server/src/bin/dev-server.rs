use std::net::SocketAddr;
use std::path::PathBuf;

use pos_server::ServerConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let bind_addr = std::env::var("HIFFEY_BIND")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 47800)));
    let db_path = std::env::var_os("HIFFEY_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/dev.db"));
    let backup_dir = db_path
        .parent()
        .map(|parent| parent.join("backups"))
        .unwrap_or_else(|| PathBuf::from("backups"));

    pos_server::run(ServerConfig::new(bind_addr, db_path, backup_dir)).await
}
