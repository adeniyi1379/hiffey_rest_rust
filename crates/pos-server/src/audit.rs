use chrono::Utc;
use sqlx::{Executor, Sqlite};
use uuid::Uuid;

/// One entry in the append-only audit trail.
pub struct AuditEntry<'a> {
    pub user_id: Option<&'a str>,
    pub terminal_id: Option<&'a str>,
    pub action: &'a str,
    pub entity: Option<&'a str>,
    pub entity_id: Option<&'a str>,
    pub details: Option<&'a str>,
}

/// Records who did what, on which terminal, and when.
///
/// Accepts a pool or a transaction so the audit row can share the change's transaction.
pub async fn record<'e, E>(executor: E, entry: AuditEntry<'_>) -> Result<(), sqlx::Error>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "INSERT INTO audit_log (id, user_id, terminal_id, action, entity, entity_id, details, at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(entry.user_id)
    .bind(entry.terminal_id)
    .bind(entry.action)
    .bind(entry.entity)
    .bind(entry.entity_id)
    .bind(entry.details)
    .bind(Utc::now().to_rfc3339())
    .execute(executor)
    .await?;
    Ok(())
}

/// Writes a write-only outbox row so the cloud copy can be built later without a backfill.
///
/// Accepts a pool or a transaction so the row is written alongside the change itself.
pub async fn enqueue<'e, E>(
    executor: E,
    entity: &str,
    entity_id: &str,
    operation: &str,
    payload: &serde_json::Value,
) -> Result<(), sqlx::Error>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "INSERT INTO sync_outbox (id, entity, entity_id, operation, payload, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(entity)
    .bind(entity_id)
    .bind(operation)
    .bind(payload.to_string())
    .bind(Utc::now().to_rfc3339())
    .execute(executor)
    .await?;
    Ok(())
}
