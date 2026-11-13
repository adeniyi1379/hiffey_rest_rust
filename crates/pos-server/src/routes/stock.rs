use std::collections::HashSet;

use axum::{
    extract::{Query, State},
    Json,
};
use chrono::Utc;
use pos_types::{
    RecordBatchRequest, RestockRequest, Role, StockBalanceDto, StockCountRequest,
    StockCountResultDto, StockLedgerEntryDto, UnitType, WastageRequest,
};
use serde::Deserialize;
use sqlx::Row;
use uuid::Uuid;

use crate::{
    audit::{self, AuditEntry},
    auth::{self, AuthUser},
    error::ApiError,
    sales, AppState,
};

fn parse_unit(value: &str) -> Result<UnitType, ApiError> {
    value
        .parse::<UnitType>()
        .map_err(|()| ApiError::internal(format!("unknown unit type in database: {value}")))
}

#[derive(Debug, Deserialize)]
pub struct LedgerQuery {
    pub item_id: Option<String>,
    pub limit: Option<i64>,
}

struct ItemInfo {
    name: String,
    unit_type: UnitType,
    low_stock_level: i64,
}

async fn item_info(state: &AppState, item_id: &str) -> Result<ItemInfo, ApiError> {
    let row = sqlx::query("SELECT name, unit_type, low_stock_level FROM items WHERE id = ?")
        .bind(item_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("Item not found."))?;
    let unit_text: String = row.get("unit_type");
    Ok(ItemInfo {
        name: row.get("name"),
        unit_type: parse_unit(&unit_text)?,
        low_stock_level: row.get("low_stock_level"),
    })
}

async fn balance_of(state: &AppState, item_id: &str) -> Result<i64, ApiError> {
    Ok(sqlx::query_scalar(
        "SELECT COALESCE((SELECT balance FROM stock_balances WHERE item_id = ?), 0)",
    )
    .bind(item_id)
    .fetch_one(&state.pool)
    .await?)
}

fn clean_reason(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn balance_dto(item_id: &str, info: &ItemInfo, balance: i64) -> StockBalanceDto {
    StockBalanceDto {
        item_id: item_id.to_owned(),
        item_name: info.name.clone(),
        unit_type: info.unit_type,
        balance,
        low_stock_level: info.low_stock_level,
    }
}

/// Every sellable item and what is on the shelf right now.
pub async fn balances(
    State(state): State<AppState>,
    actor: AuthUser,
) -> Result<Json<Vec<StockBalanceDto>>, ApiError> {
    let _ = actor;
    let rows = sqlx::query(
        "SELECT i.id, i.name, i.unit_type, i.low_stock_level,
                COALESCE(b.balance, 0) AS balance
         FROM items i
         LEFT JOIN stock_balances b ON b.item_id = i.id
         ORDER BY i.name COLLATE NOCASE",
    )
    .fetch_all(&state.pool)
    .await?;

    let mut balances = Vec::with_capacity(rows.len());
    for row in &rows {
        let unit_text: String = row.get("unit_type");
        balances.push(StockBalanceDto {
            item_id: row.get("id"),
            item_name: row.get("name"),
            unit_type: parse_unit(&unit_text)?,
            balance: row.get("balance"),
            low_stock_level: row.get("low_stock_level"),
        });
    }
    Ok(Json(balances))
}

/// Adds a batch cooked or bought into stock.
pub async fn add_batch(
    State(state): State<AppState>,
    actor: AuthUser,
    Json(request): Json<RecordBatchRequest>,
) -> Result<Json<StockBalanceDto>, ApiError> {
    auth::require(&actor, &[Role::Owner, Role::Manager])?;
    if request.quantity <= 0 {
        return Err(ApiError::bad_request("A batch must be more than zero."));
    }

    let item = sqlx::query("SELECT id, name, unit_type, low_stock_level FROM items WHERE id = ?")
        .bind(&request.item_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("Item not found."))?;

    let reason = request
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let now = Utc::now().to_rfc3339();
    let batch_id = Uuid::now_v7().to_string();

    {
        let mut pooled = state.pool.acquire().await?;
        let conn = &mut *pooled;
        sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
        let result = async {
            sales::apply_stock_movement(
                &mut *conn,
                &request.item_id,
                "batch_in",
                request.quantity,
                &batch_id,
                reason,
                &actor.user.id,
                &actor.terminal_id,
                &now,
            )
            .await?;

            audit::record(
                &mut *conn,
                AuditEntry {
                    user_id: Some(&actor.user.id),
                    terminal_id: Some(&actor.terminal_id),
                    action: "stock.batch_in",
                    entity: Some("items"),
                    entity_id: Some(&request.item_id),
                    details: Some(&format!("+{}", request.quantity)),
                },
            )
            .await?;
            audit::enqueue(
                &mut *conn,
                "stock_ledger",
                &batch_id,
                "create",
                &serde_json::json!({
                    "id": &batch_id,
                    "item_id": &request.item_id,
                    "movement_type": "batch_in",
                    "quantity": request.quantity,
                    "reason": reason,
                }),
            )
            .await?;
            Ok::<(), ApiError>(())
        }
        .await;

        match result {
            Ok(()) => {
                sqlx::query("COMMIT").execute(&mut *conn).await?;
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
                return Err(error);
            }
        }
    }

    let balance: i64 = sqlx::query_scalar(
        "SELECT COALESCE((SELECT balance FROM stock_balances WHERE item_id = ?), 0)",
    )
    .bind(&request.item_id)
    .fetch_one(&state.pool)
    .await?;

    state.events.send(
        "stock.changed",
        serde_json::json!({ "item_id": request.item_id }),
    );

    let unit_text: String = item.get("unit_type");
    Ok(Json(StockBalanceDto {
        item_id: request.item_id,
        item_name: item.get("name"),
        unit_type: parse_unit(&unit_text)?,
        balance,
        low_stock_level: item.get("low_stock_level"),
    }))
}

/// Records spoiled food, staff meals, giveaways or mistakes.
pub async fn record_wastage(
    State(state): State<AppState>,
    actor: AuthUser,
    Json(request): Json<WastageRequest>,
) -> Result<Json<StockBalanceDto>, ApiError> {
    auth::require(&actor, &[Role::Owner, Role::Manager])?;
    let reason = clean_reason(Some(&request.reason))
        .ok_or_else(|| ApiError::bad_request("Give a reason for the wastage."))?;
    if reason.chars().count() > 120 {
        return Err(ApiError::bad_request(
            "Reason must be 120 characters or fewer.",
        ));
    }
    if request.quantity <= 0 {
        return Err(ApiError::bad_request("Wastage must be more than zero."));
    }

    let info = item_info(&state, &request.item_id).await?;
    let available = balance_of(&state, &request.item_id).await?;
    if request.quantity > available {
        return Err(ApiError::bad_request(format!(
            "Only {available} of {} left in stock.",
            info.name
        )));
    }

    let now = Utc::now().to_rfc3339();
    let movement_id = Uuid::now_v7().to_string();

    move_stock(
        &state,
        &actor,
        &request.item_id,
        "wastage",
        -request.quantity,
        &movement_id,
        Some(&reason),
        "stock.wastage",
        &now,
    )
    .await?;

    let balance = balance_of(&state, &request.item_id).await?;
    state.events.send(
        "stock.changed",
        serde_json::json!({ "item_id": request.item_id }),
    );
    Ok(Json(balance_dto(&request.item_id, &info, balance)))
}

/// Brings drinks, pastries or a fresh batch back into stock.
pub async fn restock(
    State(state): State<AppState>,
    actor: AuthUser,
    Json(request): Json<RestockRequest>,
) -> Result<Json<StockBalanceDto>, ApiError> {
    auth::require(&actor, &[Role::Owner, Role::Manager])?;
    if request.quantity <= 0 {
        return Err(ApiError::bad_request("A restock must be more than zero."));
    }

    let info = item_info(&state, &request.item_id).await?;
    let reason = clean_reason(request.reason.as_deref());
    let now = Utc::now().to_rfc3339();
    let movement_id = Uuid::now_v7().to_string();

    move_stock(
        &state,
        &actor,
        &request.item_id,
        "restock",
        request.quantity,
        &movement_id,
        reason.as_deref(),
        "stock.restock",
        &now,
    )
    .await?;

    let balance = balance_of(&state, &request.item_id).await?;
    state.events.send(
        "stock.changed",
        serde_json::json!({ "item_id": request.item_id }),
    );
    Ok(Json(balance_dto(&request.item_id, &info, balance)))
}

/// Records an end-of-day count and stores the difference from the expected balance.
pub async fn count(
    State(state): State<AppState>,
    actor: AuthUser,
    Json(request): Json<StockCountRequest>,
) -> Result<Json<Vec<StockCountResultDto>>, ApiError> {
    auth::require(&actor, &[Role::Owner, Role::Manager])?;
    if request.counts.is_empty() {
        return Err(ApiError::bad_request("Count at least one item."));
    }

    let reason = clean_reason(request.reason.as_deref());
    let mut seen = HashSet::with_capacity(request.counts.len());
    for count in &request.counts {
        if count.counted_qty < 0 {
            return Err(ApiError::bad_request(
                "Counted quantities cannot be negative.",
            ));
        }
        if !seen.insert(count.item_id.as_str()) {
            return Err(ApiError::bad_request("Each item may be counted only once."));
        }
    }

    let now = Utc::now().to_rfc3339();
    let mut results = Vec::with_capacity(request.counts.len());

    {
        let mut pooled = state.pool.acquire().await?;
        let conn = &mut *pooled;
        sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;

        let outcome = async {
            for count in &request.counts {
                let row = sqlx::query(
                    "SELECT i.name, COALESCE(b.balance, 0) AS expected
                     FROM items i
                     LEFT JOIN stock_balances b ON b.item_id = i.id
                     WHERE i.id = ?",
                )
                .bind(&count.item_id)
                .fetch_optional(&mut *conn)
                .await?
                .ok_or_else(|| ApiError::not_found("Item not found."))?;

                let item_name: String = row.get("name");
                let expected_qty: i64 = row.get("expected");
                let difference = count.counted_qty - expected_qty;
                let count_id = Uuid::now_v7().to_string();

                sqlx::query(
                    "INSERT INTO stock_counts
                     (id, item_id, counted_qty, expected_qty, difference, counted_at,
                      user_id, terminal_id)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(&count_id)
                .bind(&count.item_id)
                .bind(count.counted_qty)
                .bind(expected_qty)
                .bind(difference)
                .bind(&now)
                .bind(&actor.user.id)
                .bind(&actor.terminal_id)
                .execute(&mut *conn)
                .await?;

                if difference != 0 {
                    sales::apply_stock_movement(
                        &mut *conn,
                        &count.item_id,
                        "count_diff",
                        difference,
                        &count_id,
                        reason.as_deref(),
                        &actor.user.id,
                        &actor.terminal_id,
                        &now,
                    )
                    .await?;
                    audit::enqueue(
                        &mut *conn,
                        "stock_ledger",
                        &count_id,
                        "create",
                        &serde_json::json!({
                            "id": &count_id,
                            "item_id": &count.item_id,
                            "movement_type": "count_diff",
                            "quantity": difference,
                            "reason": reason,
                        }),
                    )
                    .await?;
                }

                audit::enqueue(
                    &mut *conn,
                    "stock_counts",
                    &count_id,
                    "create",
                    &serde_json::json!({
                        "id": &count_id,
                        "item_id": &count.item_id,
                        "counted_qty": count.counted_qty,
                        "expected_qty": expected_qty,
                        "difference": difference,
                    }),
                )
                .await?;

                results.push(StockCountResultDto {
                    item_id: count.item_id.clone(),
                    item_name,
                    expected_qty,
                    counted_qty: count.counted_qty,
                    difference,
                });
            }

            audit::record(
                &mut *conn,
                AuditEntry {
                    user_id: Some(&actor.user.id),
                    terminal_id: Some(&actor.terminal_id),
                    action: "stock.counted",
                    entity: Some("stock_counts"),
                    entity_id: None,
                    details: Some(&format!("{} items", request.counts.len())),
                },
            )
            .await?;
            Ok::<(), ApiError>(())
        }
        .await;

        match outcome {
            Ok(()) => {
                sqlx::query("COMMIT").execute(&mut *conn).await?;
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
                return Err(error);
            }
        }
    }

    state.events.send("stock.changed", serde_json::json!({}));
    Ok(Json(results))
}

/// The permanent stock history. Nothing here is ever deleted.
pub async fn ledger(
    State(state): State<AppState>,
    actor: AuthUser,
    Query(query): Query<LedgerQuery>,
) -> Result<Json<Vec<StockLedgerEntryDto>>, ApiError> {
    let _ = actor;
    let limit = query.limit.unwrap_or(100).clamp(1, 500);

    let rows = sqlx::query(
        "SELECT l.id, l.item_id, i.name AS item_name, l.movement_type, l.quantity, l.reason,
                l.user_id, u.name AS user_name, l.created_at
         FROM stock_ledger l
         JOIN items i ON i.id = l.item_id
         LEFT JOIN users u ON u.id = l.user_id
         WHERE (? IS NULL OR l.item_id = ?)
         ORDER BY l.created_at DESC
         LIMIT ?",
    )
    .bind(query.item_id.as_deref())
    .bind(query.item_id.as_deref())
    .bind(limit)
    .fetch_all(&state.pool)
    .await?;

    let entries = rows
        .iter()
        .map(|row| StockLedgerEntryDto {
            id: row.get("id"),
            item_id: row.get("item_id"),
            item_name: row.get("item_name"),
            movement_type: row.get("movement_type"),
            quantity: row.get("quantity"),
            reason: row.get("reason"),
            user_id: row.get("user_id"),
            user_name: row.get("user_name"),
            created_at: row.get("created_at"),
        })
        .collect();

    Ok(Json(entries))
}

#[allow(clippy::too_many_arguments)]
async fn move_stock(
    state: &AppState,
    actor: &AuthUser,
    item_id: &str,
    movement_type: &str,
    quantity: i64,
    movement_id: &str,
    reason: Option<&str>,
    action: &str,
    now: &str,
) -> Result<(), ApiError> {
    let mut pooled = state.pool.acquire().await?;
    let conn = &mut *pooled;
    sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;

    let outcome = async {
        sales::apply_stock_movement(
            &mut *conn,
            item_id,
            movement_type,
            quantity,
            movement_id,
            reason,
            &actor.user.id,
            &actor.terminal_id,
            now,
        )
        .await?;

        audit::record(
            &mut *conn,
            AuditEntry {
                user_id: Some(&actor.user.id),
                terminal_id: Some(&actor.terminal_id),
                action,
                entity: Some("items"),
                entity_id: Some(item_id),
                details: Some(&format!("{quantity:+}")),
            },
        )
        .await?;
        audit::enqueue(
            &mut *conn,
            "stock_ledger",
            movement_id,
            "create",
            &serde_json::json!({
                "id": movement_id,
                "item_id": item_id,
                "movement_type": movement_type,
                "quantity": quantity,
                "reason": reason,
            }),
        )
        .await?;
        Ok::<(), ApiError>(())
    }
    .await;

    match outcome {
        Ok(()) => {
            sqlx::query("COMMIT").execute(&mut *conn).await?;
            Ok(())
        }
        Err(error) => {
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
            Err(error)
        }
    }
}
