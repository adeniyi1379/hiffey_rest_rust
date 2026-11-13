use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use pos_types::{CreateWorkerRequest, CreatedWorker, PinResetResponse, Role, WorkerSummary};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    audit::{self, AuditEntry},
    auth::{self, AuthUser},
    error::ApiError,
    AppState,
};

use super::require_text;

fn ensure_can_manage(actor: &AuthUser, target_role: Role) -> Result<(), ApiError> {
    auth::require(actor, &[Role::Owner, Role::Manager])?;
    if actor.user.role != Role::Owner && target_role == Role::Owner {
        return Err(ApiError::forbidden(
            "Only the owner can manage the owner account.",
        ));
    }
    Ok(())
}

fn worker_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<WorkerSummary, ApiError> {
    let role: String = row.get("role");
    let role = role
        .parse::<Role>()
        .map_err(|()| ApiError::internal(format!("unknown role in database: {role}")))?;
    Ok(WorkerSummary {
        id: row.get("id"),
        name: row.get("name"),
        role,
        active: row.get::<i64, _>("active") != 0,
        created_at: row.get("created_at"),
        pin_reset_at: row.get("pin_reset_at"),
    })
}

/// Lists the workers who can still sign in.
pub async fn list(
    State(state): State<AppState>,
    actor: AuthUser,
) -> Result<Json<Vec<WorkerSummary>>, ApiError> {
    auth::require(&actor, &[Role::Owner, Role::Manager])?;
    let rows = sqlx::query(
        "SELECT id, name, role, active, created_at, pin_reset_at
         FROM users WHERE active = 1 ORDER BY name COLLATE NOCASE",
    )
    .fetch_all(&state.pool)
    .await?;

    let workers = rows.iter().map(worker_from_row).collect::<Result<_, _>>()?;
    Ok(Json(workers))
}

/// Creates a worker and returns their generated PIN once.
pub async fn create(
    State(state): State<AppState>,
    actor: AuthUser,
    Json(request): Json<CreateWorkerRequest>,
) -> Result<(StatusCode, Json<CreatedWorker>), ApiError> {
    ensure_can_manage(&actor, request.role)?;
    let name = require_text(&request.name, "Worker name", 60)?;

    let duplicate: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM users WHERE active = 1 AND name = ? COLLATE NOCASE",
    )
    .bind(&name)
    .fetch_one(&state.pool)
    .await?;
    if duplicate > 0 {
        return Err(ApiError::conflict(
            "duplicate_name",
            "An active worker already has that name.",
        ));
    }

    let pin = auth::generate_unique_pin(&state.pool).await?;
    let pin_hash = auth::hash_pin(&pin)?;
    let worker_id = Uuid::now_v7().to_string();
    let now = Utc::now().to_rfc3339();
    let role = request.role.as_str();

    let mut tx = state.pool.begin().await?;
    sqlx::query(
        "INSERT INTO users
         (id, name, role, pin_hash, active, created_by, pin_reset_at, created_at, updated_at, terminal_id)
         VALUES (?, ?, ?, ?, 1, ?, NULL, ?, ?, ?)",
    )
    .bind(&worker_id)
    .bind(&name)
    .bind(role)
    .bind(&pin_hash)
    .bind(&actor.user.id)
    .bind(&now)
    .bind(&now)
    .bind(&actor.terminal_id)
    .execute(&mut *tx)
    .await?;

    audit::record(
        &mut *tx,
        AuditEntry {
            user_id: Some(&actor.user.id),
            terminal_id: Some(&actor.terminal_id),
            action: "worker.created",
            entity: Some("users"),
            entity_id: Some(&worker_id),
            details: Some(&name),
        },
    )
    .await?;
    audit::enqueue(
        &mut *tx,
        "users",
        &worker_id,
        "create",
        &serde_json::json!({
            "id": &worker_id,
            "name": &name,
            "role": role,
            "active": true,
        }),
    )
    .await?;
    tx.commit().await?;

    state.events.send(
        "workers.changed",
        serde_json::json!({ "action": "created" }),
    );

    Ok((
        StatusCode::CREATED,
        Json(CreatedWorker {
            worker: WorkerSummary {
                id: worker_id,
                name,
                role: request.role,
                active: true,
                created_at: now,
                pin_reset_at: None,
            },
            pin,
        }),
    ))
}

async fn active_worker(state: &AppState, id: &str) -> Result<(String, Role), ApiError> {
    let row = sqlx::query("SELECT name, role, active FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("Worker not found."))?;

    if row.get::<i64, _>("active") == 0 {
        return Err(ApiError::conflict(
            "worker_inactive",
            "That worker has already been deactivated.",
        ));
    }

    let role: String = row.get("role");
    let role = role
        .parse::<Role>()
        .map_err(|()| ApiError::internal(format!("unknown role in database: {role}")))?;
    Ok((row.get("name"), role))
}

/// Replaces a worker's PIN and signs them out everywhere.
pub async fn reset_pin(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<PinResetResponse>, ApiError> {
    let (name, target_role) = active_worker(&state, &id).await?;
    ensure_can_manage(&actor, target_role)?;

    let pin = auth::generate_unique_pin(&state.pool).await?;
    let pin_hash = auth::hash_pin(&pin)?;
    let now = Utc::now().to_rfc3339();

    let mut tx = state.pool.begin().await?;
    sqlx::query("UPDATE users SET pin_hash = ?, pin_reset_at = ?, updated_at = ? WHERE id = ?")
        .bind(&pin_hash)
        .bind(&now)
        .bind(&now)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id = ?")
        .bind(&id)
        .execute(&mut *tx)
        .await?;

    audit::record(
        &mut *tx,
        AuditEntry {
            user_id: Some(&actor.user.id),
            terminal_id: Some(&actor.terminal_id),
            action: "worker.pin_reset",
            entity: Some("users"),
            entity_id: Some(&id),
            details: Some(&name),
        },
    )
    .await?;
    audit::enqueue(
        &mut *tx,
        "users",
        &id,
        "update",
        &serde_json::json!({ "id": id, "pin_reset_at": now }),
    )
    .await?;
    tx.commit().await?;

    state.events.send(
        "workers.changed",
        serde_json::json!({ "action": "pin_reset" }),
    );

    Ok(Json(PinResetResponse { pin }))
}

/// Deactivates a worker and revokes their sessions. Records are never deleted.
pub async fn deactivate(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    if id == actor.user.id {
        return Err(ApiError::conflict(
            "cannot_deactivate_self",
            "You cannot deactivate your own account.",
        ));
    }

    let (name, target_role) = active_worker(&state, &id).await?;
    ensure_can_manage(&actor, target_role)?;
    let now = Utc::now().to_rfc3339();

    let mut tx = state.pool.begin().await?;
    sqlx::query("UPDATE users SET active = 0, updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id = ?")
        .bind(&id)
        .execute(&mut *tx)
        .await?;

    audit::record(
        &mut *tx,
        AuditEntry {
            user_id: Some(&actor.user.id),
            terminal_id: Some(&actor.terminal_id),
            action: "worker.deactivated",
            entity: Some("users"),
            entity_id: Some(&id),
            details: Some(&name),
        },
    )
    .await?;
    audit::enqueue(
        &mut *tx,
        "users",
        &id,
        "update",
        &serde_json::json!({ "id": id, "active": false }),
    )
    .await?;
    tx.commit().await?;

    state.events.send(
        "workers.changed",
        serde_json::json!({ "action": "deactivated" }),
    );

    Ok(StatusCode::NO_CONTENT)
}
