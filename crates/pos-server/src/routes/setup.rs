use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use chrono::Utc;
use pos_types::{LoginResponse, Role, SetupRequest, SetupResponse, SetupStatus, UserSummary};
use uuid::Uuid;

use crate::{
    audit::{self, AuditEntry},
    auth,
    error::ApiError,
    AppState,
};

use super::require_text;

/// Tells the first-run wizard whether this server already has an owner.
pub async fn status(State(state): State<AppState>) -> Result<Json<SetupStatus>, ApiError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.pool)
        .await?;
    let business_name: Option<String> =
        sqlx::query_scalar("SELECT NULLIF(business_name, '') FROM settings WHERE id = 1")
            .fetch_one(&state.pool)
            .await?;

    Ok(Json(SetupStatus {
        configured: count > 0,
        business_name,
        version: state.version.clone(),
    }))
}

/// Creates the business settings and the first owner account.
///
/// Only available while the database has no users. The generated owner PIN is
/// returned exactly once.
pub async fn complete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SetupRequest>,
) -> Result<(StatusCode, Json<SetupResponse>), ApiError> {
    let business_name = require_text(&request.business_name, "Business name", 120)?;
    let owner_name = require_text(&request.owner_name, "Owner name", 60)?;
    let business_address = request.business_address.trim().to_owned();
    let business_phone = request.business_phone.trim().to_owned();
    if business_address.chars().count() > 200 {
        return Err(ApiError::bad_request(
            "Business address must be 200 characters or fewer.",
        ));
    }
    if business_phone.chars().count() > 40 {
        return Err(ApiError::bad_request(
            "Business phone must be 40 characters or fewer.",
        ));
    }

    let terminal_id = auth::terminal_id(&headers);
    let pin = auth::generate_unique_pin(&state.pool).await?;
    let pin_hash = auth::hash_pin(&pin)?;
    let owner_id = Uuid::now_v7().to_string();
    let now = Utc::now().to_rfc3339();

    let mut tx = state.pool.begin().await?;
    let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&mut *tx)
        .await?;
    if existing > 0 {
        return Err(ApiError::conflict(
            "already_configured",
            "This system already has an owner account.",
        ));
    }

    sqlx::query(
        "INSERT INTO users
         (id, name, role, pin_hash, active, created_by, pin_reset_at, created_at, updated_at, terminal_id)
         VALUES (?, ?, 'owner', ?, 1, NULL, NULL, ?, ?, ?)",
    )
    .bind(&owner_id)
    .bind(&owner_name)
    .bind(&pin_hash)
    .bind(&now)
    .bind(&now)
    .bind(&terminal_id)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "UPDATE settings
         SET business_name = ?, business_address = ?, business_phone = ?, updated_at = ?, updated_by = ?
         WHERE id = 1",
    )
    .bind(&business_name)
    .bind(&business_address)
    .bind(&business_phone)
    .bind(&now)
    .bind(&owner_id)
    .execute(&mut *tx)
    .await?;

    let category_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM categories")
        .fetch_one(&mut *tx)
        .await?;
    if category_count == 0 {
        for (index, name) in ["Rice", "Sides", "Pastries", "Drinks", "Packs"]
            .iter()
            .enumerate()
        {
            let category_id = Uuid::now_v7().to_string();
            let sort_order = index as i64 + 1;
            sqlx::query(
                "INSERT INTO categories
                 (id, name, sort_order, active, created_at, updated_at, terminal_id)
                 VALUES (?, ?, ?, 1, ?, ?, ?)",
            )
            .bind(&category_id)
            .bind(name)
            .bind(sort_order)
            .bind(&now)
            .bind(&now)
            .bind(&terminal_id)
            .execute(&mut *tx)
            .await?;
            audit::enqueue(
                &mut *tx,
                "categories",
                &category_id,
                "create",
                &serde_json::json!({
                    "id": &category_id,
                    "name": name,
                    "sort_order": sort_order,
                    "active": true,
                }),
            )
            .await?;
        }
    }

    audit::record(
        &mut *tx,
        AuditEntry {
            user_id: Some(&owner_id),
            terminal_id: Some(&terminal_id),
            action: "setup.completed",
            entity: Some("business"),
            entity_id: None,
            details: Some(&business_name),
        },
    )
    .await?;
    audit::enqueue(
        &mut *tx,
        "users",
        &owner_id,
        "create",
        &serde_json::json!({
            "id": &owner_id,
            "name": &owner_name,
            "role": "owner",
            "active": true,
        }),
    )
    .await?;
    audit::enqueue(
        &mut *tx,
        "settings",
        "1",
        "update",
        &serde_json::json!({
            "business_name": business_name,
            "business_address": business_address,
            "business_phone": business_phone,
        }),
    )
    .await?;

    tx.commit().await?;

    let (token, expires_at) = auth::create_session(&state.pool, &owner_id, &terminal_id).await?;
    state.events.send(
        "setup.completed",
        serde_json::json!({ "business_name": business_name }),
    );

    Ok((
        StatusCode::CREATED,
        Json(SetupResponse {
            pin,
            session: LoginResponse {
                token,
                user: UserSummary {
                    id: owner_id,
                    name: owner_name,
                    role: Role::Owner,
                },
                expires_at,
            },
        }),
    ))
}
