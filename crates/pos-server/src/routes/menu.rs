use axum::{
    extract::{Path, State},
    http::header,
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use pos_types::MenuResponse;
use sqlx::Row;

use crate::{auth::AuthUser, catalog, error::ApiError, AppState};

/// The live menu: categories, items with today's prices, and packs.
pub async fn get(
    State(state): State<AppState>,
    actor: AuthUser,
) -> Result<Json<MenuResponse>, ApiError> {
    let _ = actor;
    let now = Utc::now().to_rfc3339();
    Ok(Json(catalog::load_menu(&state.pool, &now).await?))
}

async fn image_response(state: &AppState, table: &str, id: &str) -> Result<Response, ApiError> {
    let sql = format!("SELECT image_mime, image_bytes FROM {table} WHERE id = ?");
    let row = sqlx::query(&sql)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("Photo not found."))?;
    let mime: Option<String> = row.get("image_mime");
    let bytes: Option<Vec<u8>> = row.get("image_bytes");
    match (mime, bytes) {
        (Some(mime), Some(bytes)) if !bytes.is_empty() => Ok((
            [
                (header::CONTENT_TYPE, mime),
                (
                    header::CACHE_CONTROL,
                    "public, max-age=31536000, immutable".to_owned(),
                ),
            ],
            bytes,
        )
            .into_response()),
        _ => Err(ApiError::not_found("Photo not found.")),
    }
}

/// Serves an item photo without a session so `<img>` tags can load it.
pub async fn get_item_image(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    image_response(&state, "items", &id).await
}

/// Serves a pack photo without a session so `<img>` tags can load it.
pub async fn get_pack_image(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    image_response(&state, "packs", &id).await
}
