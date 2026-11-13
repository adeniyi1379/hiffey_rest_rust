use axum::{extract::State, Json};
use chrono::Utc;
use pos_types::{Role, SettingsDto, UpdateSettingsRequest};
use sqlx::Row;

use crate::{
    audit::{self, AuditEntry},
    auth::{self, AuthUser},
    error::ApiError,
    AppState,
};

use super::require_text;

async fn load(state: &AppState) -> Result<SettingsDto, ApiError> {
    let row = sqlx::query(
        "SELECT business_name, business_address, business_phone, receipt_footer,
                packaging_fee_kobo, low_stock_default, updated_at
         FROM settings WHERE id = 1",
    )
    .fetch_one(&state.pool)
    .await?;

    Ok(SettingsDto {
        business_name: row.get("business_name"),
        business_address: row.get("business_address"),
        business_phone: row.get("business_phone"),
        receipt_footer: row.get("receipt_footer"),
        packaging_fee_kobo: row.get("packaging_fee_kobo"),
        low_stock_default: row.get("low_stock_default"),
        updated_at: row.get("updated_at"),
    })
}

/// Business settings. Any signed-in worker may read them for the receipt header.
pub async fn get(
    State(state): State<AppState>,
    actor: AuthUser,
) -> Result<Json<SettingsDto>, ApiError> {
    let _ = actor;
    Ok(Json(load(&state).await?))
}

/// Owner and manager may change the business settings.
pub async fn update(
    State(state): State<AppState>,
    actor: AuthUser,
    Json(request): Json<UpdateSettingsRequest>,
) -> Result<Json<SettingsDto>, ApiError> {
    auth::require(&actor, &[Role::Owner, Role::Manager])?;

    let business_name = require_text(&request.business_name, "Business name", 120)?;
    let business_address = request.business_address.trim().to_owned();
    let business_phone = request.business_phone.trim().to_owned();
    let receipt_footer = request.receipt_footer.trim().to_owned();

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
    if receipt_footer.chars().count() > 200 {
        return Err(ApiError::bad_request(
            "Receipt footer must be 200 characters or fewer.",
        ));
    }
    if request.packaging_fee_kobo < 0 {
        return Err(ApiError::bad_request("Packaging fee cannot be negative."));
    }
    if request.low_stock_default < 0 {
        return Err(ApiError::bad_request(
            "Low stock threshold cannot be negative.",
        ));
    }

    let now = Utc::now().to_rfc3339();
    let mut tx = state.pool.begin().await?;
    sqlx::query(
        "UPDATE settings
         SET business_name = ?, business_address = ?, business_phone = ?,
             receipt_footer = ?, packaging_fee_kobo = ?, low_stock_default = ?,
             updated_at = ?, updated_by = ?
         WHERE id = 1",
    )
    .bind(&business_name)
    .bind(&business_address)
    .bind(&business_phone)
    .bind(&receipt_footer)
    .bind(request.packaging_fee_kobo)
    .bind(request.low_stock_default)
    .bind(&now)
    .bind(&actor.user.id)
    .execute(&mut *tx)
    .await?;

    audit::record(
        &mut *tx,
        AuditEntry {
            user_id: Some(&actor.user.id),
            terminal_id: Some(&actor.terminal_id),
            action: "settings.updated",
            entity: Some("settings"),
            entity_id: Some("1"),
            details: None,
        },
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
            "receipt_footer": receipt_footer,
            "packaging_fee_kobo": request.packaging_fee_kobo,
            "low_stock_default": request.low_stock_default,
        }),
    )
    .await?;
    tx.commit().await?;

    state
        .events
        .send("settings.updated", serde_json::json!({ "updated_at": now }));

    Ok(Json(load(&state).await?))
}
