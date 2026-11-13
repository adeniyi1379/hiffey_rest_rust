use std::net::SocketAddr;
use std::path::PathBuf;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            if server_enabled() {
                start_server(app.handle());
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while building tauri application");
}

fn server_enabled() -> bool {
    std::env::var("HIFFEY_SERVER")
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn start_server(app: &tauri::AppHandle) {
    let data_dir = match app.path().app_data_dir() {
        Ok(dir) => dir,
        Err(error) => {
            log::error!("cannot resolve the app data folder: {error}");
            return;
        }
    };
    let db_path = std::env::var_os("HIFFEY_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|| data_dir.join("hiffey-pos.db"));
    let backup_dir = data_dir.join("backups");
    let bind_addr = std::env::var("HIFFEY_BIND")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| SocketAddr::from(([0, 0, 0, 0], 47800)));

    log::info!(
        "starting main till server on {bind_addr} with database {}",
        db_path.display()
    );
    tauri::async_runtime::spawn(async move {
        let config = pos_server::ServerConfig::new(bind_addr, &db_path, &backup_dir);
        if let Err(error) = pos_server::run(config).await {
            log::error!("main till server stopped: {error}");
        }
    });
}
