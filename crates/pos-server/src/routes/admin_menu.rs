use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header, HeaderMap},
    Json,
};
use chrono::{DateTime, Utc};
use image::{codecs::jpeg::JpegEncoder, imageops::FilterType, DynamicImage, Rgba, RgbaImage};
use pos_types::{
    CreateCategoryRequest, CreateItemRequest, CreatePackRequest, MenuResponse, PackComponentInput,
    PackPricingMode, Role, UpdateCategoryRequest, UpdateItemRequest, UpdatePackRequest,
};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::{
    audit::{self, AuditEntry},
    auth::{self, AuthUser},
    catalog,
    error::ApiError,
    AppState,
};

use super::require_text;

fn parse_effective(value: &Option<String>) -> Result<DateTime<Utc>, ApiError> {
    match value {
        None => Ok(Utc::now()),
        Some(text) => DateTime::parse_from_rfc3339(text)
            .map(|stamp| stamp.with_timezone(&Utc))
            .map_err(|_| {
                ApiError::bad_request(
                    "Effective time must be an ISO 8601 date and time, e.g. 2026-11-12T08:00:00+01:00.",
                )
            }),
    }
}

async fn name_taken(
    pool: &SqlitePool,
    sql: &'static str,
    name: &str,
    exclude_id: Option<&str>,
) -> Result<bool, ApiError> {
    let count: i64 = sqlx::query_scalar(sql)
        .bind(name)
        .bind(exclude_id)
        .fetch_one(pool)
        .await?;
    Ok(count > 0)
}

fn check_money(value: i64, field: &str) -> Result<(), ApiError> {
    if value < 0 {
        return Err(ApiError::bad_request(format!(
            "{field} cannot be negative."
        )));
    }
    Ok(())
}

fn validate_components(components: &[PackComponentInput]) -> Result<Vec<(String, i64)>, ApiError> {
    if components.is_empty() {
        return Err(ApiError::bad_request("A pack needs at least one item."));
    }
    let mut cleaned: Vec<(String, i64)> = Vec::with_capacity(components.len());
    for component in components {
        if component.quantity <= 0 {
            return Err(ApiError::bad_request(
                "Every pack quantity must be at least 1.",
            ));
        }
        if cleaned
            .iter()
            .any(|(item_id, _)| item_id == &component.item_id)
        {
            return Err(ApiError::bad_request(
                "Each item can appear only once in a pack.",
            ));
        }
        cleaned.push((component.item_id.clone(), component.quantity));
    }
    Ok(cleaned)
}

fn validate_fixed_price(
    mode: PackPricingMode,
    fixed_price_kobo: Option<i64>,
) -> Result<Option<i64>, ApiError> {
    match mode {
        PackPricingMode::Fixed => match fixed_price_kobo {
            Some(value) if value >= 0 => Ok(Some(value)),
            Some(_) => Err(ApiError::bad_request(
                "Fixed pack price cannot be negative.",
            )),
            None => Err(ApiError::bad_request("A fixed-price pack needs a price.")),
        },
        PackPricingMode::Calculated => Ok(None),
    }
}

async fn ensure_items_exist(
    pool: &SqlitePool,
    components: &[(String, i64)],
) -> Result<(), ApiError> {
    for (item_id, _) in components {
        let found: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM items WHERE id = ?")
            .bind(item_id)
            .fetch_one(pool)
            .await?;
        if found == 0 {
            return Err(ApiError::bad_request(
                "One of the selected items no longer exists.",
            ));
        }
    }
    Ok(())
}

async fn insert_components(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    pack_id: &str,
    components: &[(String, i64)],
    now: &str,
    terminal_id: &str,
) -> Result<(), ApiError> {
    for (item_id, quantity) in components {
        sqlx::query(
            "INSERT INTO pack_components
             (id, pack_id, item_id, quantity, created_at, updated_at, terminal_id)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(Uuid::now_v7().to_string())
        .bind(pack_id)
        .bind(item_id)
        .bind(quantity)
        .bind(now)
        .bind(now)
        .bind(terminal_id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn refreshed(state: &AppState) -> Result<Json<MenuResponse>, ApiError> {
    let now = Utc::now().to_rfc3339();
    Ok(Json(catalog::load_menu(&state.pool, &now).await?))
}

fn changed(state: &AppState, action: &str, entity: &str) {
    state.events.send(
        "menu.changed",
        serde_json::json!({ "action": action, "entity": entity }),
    );
}

/// Creates a category (MENU-3).
pub async fn create_category(
    State(state): State<AppState>,
    actor: AuthUser,
    Json(request): Json<CreateCategoryRequest>,
) -> Result<Json<MenuResponse>, ApiError> {
    auth::require(&actor, &[Role::Owner, Role::Manager])?;
    let name = require_text(&request.name, "Category name", 60)?;
    if request.sort_order < 0 {
        return Err(ApiError::bad_request("Sort order cannot be negative."));
    }
    if name_taken(
        &state.pool,
        "SELECT COUNT(*) FROM categories WHERE name = ? COLLATE NOCASE AND id != COALESCE(?, '')",
        &name,
        None,
    )
    .await?
    {
        return Err(ApiError::conflict(
            "duplicate_name",
            "A category with that name already exists.",
        ));
    }

    let id = Uuid::now_v7().to_string();
    let now = Utc::now().to_rfc3339();
    let mut tx = state.pool.begin().await?;
    sqlx::query(
        "INSERT INTO categories (id, name, sort_order, active, created_at, updated_at, terminal_id)
         VALUES (?, ?, ?, 1, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(request.sort_order)
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
            action: "category.created",
            entity: Some("categories"),
            entity_id: Some(&id),
            details: Some(&name),
        },
    )
    .await?;
    audit::enqueue(
        &mut *tx,
        "categories",
        &id,
        "create",
        &serde_json::json!({ "id": &id, "name": &name, "sort_order": request.sort_order, "active": true }),
    )
    .await?;
    tx.commit().await?;

    changed(&state, "created", "categories");
    refreshed(&state).await
}

/// Renames a category or changes its order and active flag.
pub async fn update_category(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
    Json(request): Json<UpdateCategoryRequest>,
) -> Result<Json<MenuResponse>, ApiError> {
    auth::require(&actor, &[Role::Owner, Role::Manager])?;
    let name = require_text(&request.name, "Category name", 60)?;
    if request.sort_order < 0 {
        return Err(ApiError::bad_request("Sort order cannot be negative."));
    }
    let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM categories WHERE id = ?")
        .bind(&id)
        .fetch_one(&state.pool)
        .await?;
    if exists == 0 {
        return Err(ApiError::not_found("Category not found."));
    }
    if name_taken(
        &state.pool,
        "SELECT COUNT(*) FROM categories WHERE name = ? COLLATE NOCASE AND id != COALESCE(?, '')",
        &name,
        Some(&id),
    )
    .await?
    {
        return Err(ApiError::conflict(
            "duplicate_name",
            "A category with that name already exists.",
        ));
    }

    let now = Utc::now().to_rfc3339();
    let mut tx = state.pool.begin().await?;
    sqlx::query(
        "UPDATE categories SET name = ?, sort_order = ?, active = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&name)
    .bind(request.sort_order)
    .bind(i64::from(request.active))
    .bind(&now)
    .bind(&id)
    .execute(&mut *tx)
    .await?;
    audit::record(
        &mut *tx,
        AuditEntry {
            user_id: Some(&actor.user.id),
            terminal_id: Some(&actor.terminal_id),
            action: "category.updated",
            entity: Some("categories"),
            entity_id: Some(&id),
            details: Some(&name),
        },
    )
    .await?;
    audit::enqueue(
        &mut *tx,
        "categories",
        &id,
        "update",
        &serde_json::json!({ "id": &id, "name": &name, "sort_order": request.sort_order, "active": request.active }),
    )
    .await?;
    tx.commit().await?;

    changed(&state, "updated", "categories");
    refreshed(&state).await
}

/// Creates an item with its starting price (MENU-1, MENU-4).
pub async fn create_item(
    State(state): State<AppState>,
    actor: AuthUser,
    Json(request): Json<CreateItemRequest>,
) -> Result<Json<MenuResponse>, ApiError> {
    auth::require(&actor, &[Role::Owner, Role::Manager])?;
    let name = require_text(&request.name, "Item name", 80)?;
    check_money(request.price_per_unit_kobo, "Price")?;
    check_money(request.low_stock_level, "Low stock level")?;

    let category: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM categories WHERE id = ?")
        .bind(&request.category_id)
        .fetch_one(&state.pool)
        .await?;
    if category == 0 {
        return Err(ApiError::bad_request(
            "Choose an existing category for this item.",
        ));
    }
    if name_taken(
        &state.pool,
        "SELECT COUNT(*) FROM items WHERE name = ? COLLATE NOCASE AND id != COALESCE(?, '')",
        &name,
        None,
    )
    .await?
    {
        return Err(ApiError::conflict(
            "duplicate_name",
            "An item with that name already exists.",
        ));
    }
    if request.effective_from.is_some() {
        return Err(ApiError::bad_request(
            "Create the item with today's price, then schedule future price changes.",
        ));
    }

    let id = Uuid::now_v7().to_string();
    let now = Utc::now().to_rfc3339();
    let unit = request.unit_type.as_str();
    let mut tx = state.pool.begin().await?;
    sqlx::query(
        "INSERT INTO items
         (id, name, category_id, unit_type, price_per_unit_kobo, low_stock_level, active,
          created_at, updated_at, terminal_id)
         VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(&request.category_id)
    .bind(unit)
    .bind(request.price_per_unit_kobo)
    .bind(request.low_stock_level)
    .bind(&now)
    .bind(&now)
    .bind(&actor.terminal_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO item_prices
         (id, item_id, price_kobo, effective_from, created_at, created_by, terminal_id)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(&id)
    .bind(request.price_per_unit_kobo)
    .bind(&now)
    .bind(&now)
    .bind(&actor.user.id)
    .bind(&actor.terminal_id)
    .execute(&mut *tx)
    .await?;
    audit::record(
        &mut *tx,
        AuditEntry {
            user_id: Some(&actor.user.id),
            terminal_id: Some(&actor.terminal_id),
            action: "item.created",
            entity: Some("items"),
            entity_id: Some(&id),
            details: Some(&name),
        },
    )
    .await?;
    audit::enqueue(
        &mut *tx,
        "items",
        &id,
        "create",
        &serde_json::json!({
            "id": &id,
            "name": &name,
            "category_id": &request.category_id,
            "unit_type": unit,
            "price_per_unit_kobo": request.price_per_unit_kobo,
            "low_stock_level": request.low_stock_level,
            "active": true,
        }),
    )
    .await?;
    tx.commit().await?;

    changed(&state, "created", "items");
    refreshed(&state).await
}

/// Edits an item. A changed price is recorded with its effective time (MENU-6).
pub async fn update_item(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
    Json(request): Json<UpdateItemRequest>,
) -> Result<Json<MenuResponse>, ApiError> {
    auth::require(&actor, &[Role::Owner, Role::Manager])?;
    let name = require_text(&request.name, "Item name", 80)?;
    check_money(request.price_per_unit_kobo, "Price")?;
    check_money(request.low_stock_level, "Low stock level")?;

    let row = sqlx::query("SELECT price_per_unit_kobo FROM items WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("Item not found."))?;
    let previous_price: i64 = row.get("price_per_unit_kobo");

    let category: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM categories WHERE id = ?")
        .bind(&request.category_id)
        .fetch_one(&state.pool)
        .await?;
    if category == 0 {
        return Err(ApiError::bad_request(
            "Choose an existing category for this item.",
        ));
    }
    if name_taken(
        &state.pool,
        "SELECT COUNT(*) FROM items WHERE name = ? COLLATE NOCASE AND id != COALESCE(?, '')",
        &name,
        Some(&id),
    )
    .await?
    {
        return Err(ApiError::conflict(
            "duplicate_name",
            "An item with that name already exists.",
        ));
    }

    let effective = parse_effective(&request.effective_from)?;
    let effective_text = effective.to_rfc3339();
    let now = Utc::now().to_rfc3339();
    let price_changed = request.price_per_unit_kobo != previous_price;
    let price_applies_now = price_changed && effective <= Utc::now();
    let unit = request.unit_type.as_str();

    let mut tx = state.pool.begin().await?;
    sqlx::query(
        "UPDATE items
         SET name = ?, category_id = ?, unit_type = ?, low_stock_level = ?, active = ?,
             updated_at = ?, price_per_unit_kobo = CASE WHEN ? THEN ? ELSE price_per_unit_kobo END
         WHERE id = ?",
    )
    .bind(&name)
    .bind(&request.category_id)
    .bind(unit)
    .bind(request.low_stock_level)
    .bind(i64::from(request.active))
    .bind(&now)
    .bind(i64::from(price_applies_now))
    .bind(request.price_per_unit_kobo)
    .bind(&id)
    .execute(&mut *tx)
    .await?;

    if price_changed {
        sqlx::query(
            "INSERT INTO item_prices
             (id, item_id, price_kobo, effective_from, created_at, created_by, terminal_id)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(Uuid::now_v7().to_string())
        .bind(&id)
        .bind(request.price_per_unit_kobo)
        .bind(&effective_text)
        .bind(&now)
        .bind(&actor.user.id)
        .bind(&actor.terminal_id)
        .execute(&mut *tx)
        .await?;
    }

    audit::record(
        &mut *tx,
        AuditEntry {
            user_id: Some(&actor.user.id),
            terminal_id: Some(&actor.terminal_id),
            action: "item.updated",
            entity: Some("items"),
            entity_id: Some(&id),
            details: Some(&name),
        },
    )
    .await?;
    audit::enqueue(
        &mut *tx,
        "items",
        &id,
        "update",
        &serde_json::json!({
            "id": &id,
            "name": &name,
            "category_id": &request.category_id,
            "unit_type": unit,
            "price_per_unit_kobo": request.price_per_unit_kobo,
            "price_effective_from": &effective_text,
            "low_stock_level": request.low_stock_level,
            "active": request.active,
        }),
    )
    .await?;
    tx.commit().await?;

    changed(&state, "updated", "items");
    refreshed(&state).await
}

/// Creates a pack of items (PACK-1, PACK-2).
pub async fn create_pack(
    State(state): State<AppState>,
    actor: AuthUser,
    Json(request): Json<CreatePackRequest>,
) -> Result<Json<MenuResponse>, ApiError> {
    auth::require(&actor, &[Role::Owner, Role::Manager])?;
    let name = require_text(&request.name, "Pack name", 80)?;
    let components = validate_components(&request.components)?;
    let fixed_price = validate_fixed_price(request.pricing_mode, request.fixed_price_kobo)?;
    ensure_items_exist(&state.pool, &components).await?;
    if name_taken(
        &state.pool,
        "SELECT COUNT(*) FROM packs WHERE name = ? COLLATE NOCASE AND id != COALESCE(?, '')",
        &name,
        None,
    )
    .await?
    {
        return Err(ApiError::conflict(
            "duplicate_name",
            "A pack with that name already exists.",
        ));
    }

    let id = Uuid::now_v7().to_string();
    let now = Utc::now().to_rfc3339();
    let mode = request.pricing_mode.as_str();
    let mut tx = state.pool.begin().await?;
    sqlx::query(
        "INSERT INTO packs
         (id, name, pricing_mode, fixed_price_kobo, active, created_at, updated_at, terminal_id)
         VALUES (?, ?, ?, ?, 1, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(mode)
    .bind(fixed_price)
    .bind(&now)
    .bind(&now)
    .bind(&actor.terminal_id)
    .execute(&mut *tx)
    .await?;
    insert_components(&mut tx, &id, &components, &now, &actor.terminal_id).await?;

    audit::record(
        &mut *tx,
        AuditEntry {
            user_id: Some(&actor.user.id),
            terminal_id: Some(&actor.terminal_id),
            action: "pack.created",
            entity: Some("packs"),
            entity_id: Some(&id),
            details: Some(&name),
        },
    )
    .await?;
    audit::enqueue(
        &mut *tx,
        "packs",
        &id,
        "create",
        &serde_json::json!({
            "id": &id,
            "name": &name,
            "pricing_mode": mode,
            "fixed_price_kobo": fixed_price,
            "active": true,
            "components": components
                .iter()
                .map(|(item_id, quantity)| serde_json::json!({ "item_id": item_id, "quantity": quantity }))
                .collect::<Vec<_>>(),
        }),
    )
    .await?;
    tx.commit().await?;

    changed(&state, "created", "packs");
    refreshed(&state).await
}

/// Replaces a pack's contents, price mode and active flag (PACK-7).
pub async fn update_pack(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
    Json(request): Json<UpdatePackRequest>,
) -> Result<Json<MenuResponse>, ApiError> {
    auth::require(&actor, &[Role::Owner, Role::Manager])?;
    let name = require_text(&request.name, "Pack name", 80)?;
    let components = validate_components(&request.components)?;
    let fixed_price = validate_fixed_price(request.pricing_mode, request.fixed_price_kobo)?;
    ensure_items_exist(&state.pool, &components).await?;

    let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM packs WHERE id = ?")
        .bind(&id)
        .fetch_one(&state.pool)
        .await?;
    if exists == 0 {
        return Err(ApiError::not_found("Pack not found."));
    }
    if name_taken(
        &state.pool,
        "SELECT COUNT(*) FROM packs WHERE name = ? COLLATE NOCASE AND id != COALESCE(?, '')",
        &name,
        Some(&id),
    )
    .await?
    {
        return Err(ApiError::conflict(
            "duplicate_name",
            "A pack with that name already exists.",
        ));
    }

    let now = Utc::now().to_rfc3339();
    let mode = request.pricing_mode.as_str();
    let mut tx = state.pool.begin().await?;
    sqlx::query(
        "UPDATE packs
         SET name = ?, pricing_mode = ?, fixed_price_kobo = ?, active = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(&name)
    .bind(mode)
    .bind(fixed_price)
    .bind(i64::from(request.active))
    .bind(&now)
    .bind(&id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM pack_components WHERE pack_id = ?")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    insert_components(&mut tx, &id, &components, &now, &actor.terminal_id).await?;

    audit::record(
        &mut *tx,
        AuditEntry {
            user_id: Some(&actor.user.id),
            terminal_id: Some(&actor.terminal_id),
            action: "pack.updated",
            entity: Some("packs"),
            entity_id: Some(&id),
            details: Some(&name),
        },
    )
    .await?;
    audit::enqueue(
        &mut *tx,
        "packs",
        &id,
        "update",
        &serde_json::json!({
            "id": &id,
            "name": &name,
            "pricing_mode": mode,
            "fixed_price_kobo": fixed_price,
            "active": request.active,
            "components": components
                .iter()
                .map(|(item_id, quantity)| serde_json::json!({ "item_id": item_id, "quantity": quantity }))
                .collect::<Vec<_>>(),
        }),
    )
    .await?;
    tx.commit().await?;

    changed(&state, "updated", "packs");
    refreshed(&state).await
}

/// Duplicates a pack under a free "(copy)" name.
pub async fn duplicate_pack(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<MenuResponse>, ApiError> {
    auth::require(&actor, &[Role::Owner, Role::Manager])?;

    let source = sqlx::query(
        "SELECT name, pricing_mode, fixed_price_kobo, active, image_mime, image_bytes
         FROM packs WHERE id = ?",
    )
    .bind(&id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("Pack not found."))?;

    let base_name: String = source.get("name");
    let mode_text: String = source.get("pricing_mode");
    let fixed_price: Option<i64> = source.get("fixed_price_kobo");
    let image_mime: Option<String> = source.get("image_mime");
    let image_bytes: Option<Vec<u8>> = source.get("image_bytes");

    let component_rows =
        sqlx::query("SELECT item_id, quantity FROM pack_components WHERE pack_id = ?")
            .bind(&id)
            .fetch_all(&state.pool)
            .await?;
    let components: Vec<(String, i64)> = component_rows
        .iter()
        .map(|row| (row.get("item_id"), row.get("quantity")))
        .collect();
    if components.is_empty() {
        return Err(ApiError::bad_request("That pack has no items to copy."));
    }

    let mut copy_name = format!("{base_name} (copy)");
    let mut attempt = 2;
    while name_taken(
        &state.pool,
        "SELECT COUNT(*) FROM packs WHERE name = ? COLLATE NOCASE AND id != COALESCE(?, '')",
        &copy_name,
        None,
    )
    .await?
    {
        copy_name = format!("{base_name} (copy {attempt})");
        attempt += 1;
        if attempt > 50 {
            return Err(ApiError::conflict(
                "duplicate_name",
                "Could not find a free name for the copied pack.",
            ));
        }
    }

    let new_id = Uuid::now_v7().to_string();
    let now = Utc::now().to_rfc3339();
    let image_updated_at = image_mime
        .as_ref()
        .and(image_bytes.as_ref())
        .map(|_| now.clone());
    let mut tx = state.pool.begin().await?;
    sqlx::query(
        "INSERT INTO packs
         (id, name, pricing_mode, fixed_price_kobo, active, created_at, updated_at, terminal_id,
          image_mime, image_bytes, image_updated_at)
         VALUES (?, ?, ?, ?, 1, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&new_id)
    .bind(&copy_name)
    .bind(&mode_text)
    .bind(fixed_price)
    .bind(&now)
    .bind(&now)
    .bind(&actor.terminal_id)
    .bind(image_mime)
    .bind(image_bytes)
    .bind(image_updated_at)
    .execute(&mut *tx)
    .await?;
    insert_components(&mut tx, &new_id, &components, &now, &actor.terminal_id).await?;

    audit::record(
        &mut *tx,
        AuditEntry {
            user_id: Some(&actor.user.id),
            terminal_id: Some(&actor.terminal_id),
            action: "pack.duplicated",
            entity: Some("packs"),
            entity_id: Some(&new_id),
            details: Some(&copy_name),
        },
    )
    .await?;
    audit::enqueue(
        &mut *tx,
        "packs",
        &new_id,
        "create",
        &serde_json::json!({
            "id": &new_id,
            "name": &copy_name,
            "pricing_mode": mode_text,
            "fixed_price_kobo": fixed_price,
            "active": true,
            "copied_from": &id,
        }),
    )
    .await?;
    tx.commit().await?;

    changed(&state, "duplicated", "packs");
    refreshed(&state).await
}

#[derive(Clone, Copy)]
enum ImageOwner {
    Item,
    Pack,
}

impl ImageOwner {
    fn table(self) -> &'static str {
        match self {
            Self::Item => "items",
            Self::Pack => "packs",
        }
    }

    fn entity(self) -> &'static str {
        match self {
            Self::Item => "items",
            Self::Pack => "packs",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Item => "Item",
            Self::Pack => "Pack",
        }
    }

    fn upload_action(self) -> &'static str {
        match self {
            Self::Item => "item.image_uploaded",
            Self::Pack => "pack.image_uploaded",
        }
    }

    fn delete_action(self) -> &'static str {
        match self {
            Self::Item => "item.image_deleted",
            Self::Pack => "pack.image_deleted",
        }
    }
}

fn photo_mime(headers: &HeaderMap) -> Result<(), ApiError> {
    let value = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let mime = value.split(';').next().unwrap_or_default().trim();
    if mime.eq_ignore_ascii_case("image/jpeg")
        || mime.eq_ignore_ascii_case("image/jpg")
        || mime.eq_ignore_ascii_case("image/png")
        || mime.eq_ignore_ascii_case("image/webp")
    {
        Ok(())
    } else {
        Err(ApiError::bad_request("Choose a JPEG, PNG or WebP photo."))
    }
}

fn process_photo(bytes: &[u8]) -> Result<Vec<u8>, ApiError> {
    let photo = image::load_from_memory(bytes).map_err(|_| {
        ApiError::bad_request("That photo could not be read. Choose a JPEG, PNG or WebP image.")
    })?;

    let photo = if photo.width() > 1200 || photo.height() > 1200 {
        photo.resize(1200, 1200, FilterType::Lanczos3)
    } else {
        photo
    };

    let rgba = photo.to_rgba8();
    let mut flattened =
        RgbaImage::from_pixel(rgba.width(), rgba.height(), Rgba([255, 255, 255, 255]));
    image::imageops::overlay(&mut flattened, &rgba, 0, 0);
    let rgb = DynamicImage::ImageRgba8(flattened).to_rgb8();

    let mut encoded = Vec::new();
    JpegEncoder::new_with_quality(&mut encoded, 82)
        .encode_image(&rgb)
        .map_err(|_| ApiError::internal("The photo could not be prepared."))?;
    Ok(encoded)
}

async fn save_photo(
    state: &AppState,
    actor: &AuthUser,
    owner: ImageOwner,
    id: &str,
    photo: Option<&[u8]>,
) -> Result<(), ApiError> {
    let exists_sql = format!("SELECT COUNT(*) FROM {} WHERE id = ?", owner.table());
    let exists: i64 = sqlx::query_scalar(&exists_sql)
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    if exists == 0 {
        return Err(ApiError::not_found(format!("{} not found.", owner.label())));
    }

    let now = Utc::now().to_rfc3339();
    let mut tx = state.pool.begin().await?;
    let action = if let Some(bytes) = photo {
        let update_sql = format!(
            "UPDATE {} SET image_mime = 'image/jpeg', image_bytes = ?, image_updated_at = ? WHERE id = ?",
            owner.table()
        );
        sqlx::query(&update_sql)
            .bind(bytes)
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        owner.upload_action()
    } else {
        let update_sql = format!(
            "UPDATE {} SET image_mime = NULL, image_bytes = NULL, image_updated_at = NULL WHERE id = ?",
            owner.table()
        );
        sqlx::query(&update_sql).bind(id).execute(&mut *tx).await?;
        owner.delete_action()
    };

    let details = photo.map_or_else(
        || "Photo removed".to_owned(),
        |bytes| format!("{} bytes", bytes.len()),
    );
    audit::record(
        &mut *tx,
        AuditEntry {
            user_id: Some(&actor.user.id),
            terminal_id: Some(&actor.terminal_id),
            action,
            entity: Some(owner.entity()),
            entity_id: Some(id),
            details: Some(&details),
        },
    )
    .await?;
    tx.commit().await?;

    changed(
        state,
        if photo.is_some() {
            "photo_updated"
        } else {
            "photo_removed"
        },
        owner.entity(),
    );
    Ok(())
}

/// Stores an item photo, resized and re-encoded as JPEG.
pub async fn upload_item_image(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<MenuResponse>, ApiError> {
    auth::require(&actor, &[Role::Owner, Role::Manager])?;
    photo_mime(&headers)?;
    let bytes = tokio::task::spawn_blocking(move || process_photo(&body))
        .await
        .map_err(|_| ApiError::internal("The photo could not be processed."))??;
    save_photo(&state, &actor, ImageOwner::Item, &id, Some(&bytes)).await?;
    refreshed(&state).await
}

/// Removes an item photo.
pub async fn delete_item_image(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<MenuResponse>, ApiError> {
    auth::require(&actor, &[Role::Owner, Role::Manager])?;
    save_photo(&state, &actor, ImageOwner::Item, &id, None).await?;
    refreshed(&state).await
}

/// Stores a pack photo, resized and re-encoded as JPEG.
pub async fn upload_pack_image(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<MenuResponse>, ApiError> {
    auth::require(&actor, &[Role::Owner, Role::Manager])?;
    photo_mime(&headers)?;
    let bytes = tokio::task::spawn_blocking(move || process_photo(&body))
        .await
        .map_err(|_| ApiError::internal("The photo could not be processed."))??;
    save_photo(&state, &actor, ImageOwner::Pack, &id, Some(&bytes)).await?;
    refreshed(&state).await
}

/// Removes a pack photo.
pub async fn delete_pack_image(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<MenuResponse>, ApiError> {
    auth::require(&actor, &[Role::Owner, Role::Manager])?;
    save_photo(&state, &actor, ImageOwner::Pack, &id, None).await?;
    refreshed(&state).await
}
