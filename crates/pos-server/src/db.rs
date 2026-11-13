use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use chrono::Utc;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    SqlitePool,
};

/// Opens (or creates) the shop database in WAL mode.
pub async fn connect(path: &Path) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));

    SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await
}

/// Opens a single-connection in-memory database, used by tests.
pub async fn connect_memory() -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename(":memory:")
        .create_if_missing(true)
        .foreign_keys(true);

    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
}

/// Applies the versioned migrations embedded in the binary.
pub async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}

/// Copies the database aside before migrations run at server start.
///
/// Returns the backup path when a backup was made. A missing database (first run)
/// is not an error.
pub fn backup_before_start(
    db_path: &Path,
    backup_dir: &Path,
) -> Result<Option<PathBuf>, std::io::Error> {
    if !db_path.exists() {
        return Ok(None);
    }

    std::fs::create_dir_all(backup_dir)?;
    let stamp = Utc::now().format("%Y%m%d-%H%M%S");
    let backup_path = backup_dir.join(format!("startup-{stamp}.db"));
    std::fs::copy(db_path, &backup_path)?;
    Ok(Some(backup_path))
}
