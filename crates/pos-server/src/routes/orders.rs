use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Duration, Utc};
use pos_core::{format_order_no, order_total, Money};
use pos_types::{
    CreateOrderRequest, OrderDto, OrderLineInput, OrderStatus, OrderType, PayOrderRequest,
    PayOrderResponse, UpdateOrderRequest, VoidOrderRequest,
};
use sqlx::SqliteConnection;
use uuid::Uuid;

use crate::{
    audit::{self, AuditEntry},
    auth::{self, AuthUser},
    error::ApiError,
    sales::{self, PreparedLine},
    AppState,
};

fn wat_day(now: DateTime<Utc>) -> String {
    (now + Duration::hours(1)).format("%Y%m%d").to_string()
}

fn table_for(
    request_type: OrderType,
    table_no: &Option<String>,
) -> Result<Option<String>, ApiError> {
    match request_type {
        OrderType::DineIn => {
            let table = table_no
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    ApiError::bad_request("A table number is required for dine-in orders.")
                })?;
            if table.chars().count() > 20 {
                return Err(ApiError::bad_request(
                    "Table number must be 20 characters or fewer.",
                ));
            }
            Ok(Some(table.to_owned()))
        }
        OrderType::Takeaway => Ok(None),
    }
}

async fn packaging_fee(
    conn: &mut SqliteConnection,
    order_type: OrderType,
) -> Result<i64, ApiError> {
    if order_type == OrderType::Takeaway {
        let fee: i64 = sqlx::query_scalar("SELECT packaging_fee_kobo FROM settings WHERE id = 1")
            .fetch_one(&mut *conn)
            .await?;
        Ok(fee)
    } else {
        Ok(0)
    }
}

async fn insert_lines(
    conn: &mut SqliteConnection,
    order_id: &str,
    lines: &[PreparedLine],
    now: &str,
) -> Result<(), ApiError> {
    for (index, line) in lines.iter().enumerate() {
        let line_id = Uuid::now_v7().to_string();
        let line_no = index as i64 + 1;
        sqlx::query(
            "INSERT INTO order_lines
             (id, order_id, line_no, kind, item_id, pack_id, name, quantity,
              unit_price_kobo, line_total_kobo, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&line_id)
        .bind(order_id)
        .bind(line_no)
        .bind(line.kind)
        .bind(&line.item_id)
        .bind(&line.pack_id)
        .bind(&line.name)
        .bind(line.quantity)
        .bind(line.unit_price_kobo)
        .bind(line.line_total_kobo)
        .bind(now)
        .bind(now)
        .execute(&mut *conn)
        .await?;

        for component in &line.components {
            sqlx::query(
                "INSERT INTO order_line_components
                 (id, order_line_id, item_id, item_name, unit_type, quantity, unit_price_kobo)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(Uuid::now_v7().to_string())
            .bind(&line_id)
            .bind(&component.item_id)
            .bind(&component.item_name)
            .bind(component.unit_type.as_str())
            .bind(component.quantity)
            .bind(component.unit_price_kobo)
            .execute(&mut *conn)
            .await?;
        }
    }
    Ok(())
}

async fn load_status(conn: &mut SqliteConnection, id: &str) -> Result<OrderStatus, ApiError> {
    let text: String = sqlx::query_scalar("SELECT status FROM orders WHERE id = ?")
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| ApiError::not_found("Order not found."))?;
    text.parse::<OrderStatus>()
        .map_err(|()| ApiError::internal(format!("unknown order status: {text}")))
}

/// Takes a new order and holds it until payment.
pub async fn create(
    State(state): State<AppState>,
    actor: AuthUser,
    Json(request): Json<CreateOrderRequest>,
) -> Result<(StatusCode, Json<OrderDto>), ApiError> {
    let table_no = table_for(request.order_type, &request.table_no)?;
    let now = Utc::now();
    let now_text = now.to_rfc3339();
    let day = wat_day(now);

    let order_id = {
        let mut pooled = state.pool.acquire().await?;
        let conn = &mut *pooled;
        sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
        match create_inner(
            conn,
            &actor,
            request.order_type,
            &table_no,
            &request.lines,
            &now_text,
            &day,
        )
        .await
        {
            Ok(order_id) => {
                sqlx::query("COMMIT").execute(&mut *conn).await?;
                order_id
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
                return Err(error);
            }
        }
    };

    let order = sales::load_order(&state.pool, &order_id).await?;
    state.events.send(
        "order.created",
        serde_json::json!({ "order_id": order_id, "order_no": order.order_no }),
    );
    Ok((StatusCode::CREATED, Json(order)))
}

#[allow(clippy::too_many_arguments)]
async fn create_inner(
    conn: &mut SqliteConnection,
    actor: &AuthUser,
    order_type: OrderType,
    table_no: &Option<String>,
    inputs: &[OrderLineInput],
    now: &str,
    day: &str,
) -> Result<String, ApiError> {
    let (lines, subtotal) = sales::prepare_lines(conn, inputs, now).await?;
    let fee = packaging_fee(conn, order_type).await?;
    let total = order_total(
        &lines
            .iter()
            .map(|line| Money::from_kobo(line.line_total_kobo))
            .collect::<Vec<_>>(),
        Money::from_kobo(fee),
    )
    .ok_or_else(|| ApiError::bad_request("That order is too large to price."))?;

    let sequence: i64 = sqlx::query_scalar(
        "INSERT INTO order_seq (day, last_seq) VALUES (?, 1)
         ON CONFLICT (day) DO UPDATE SET last_seq = last_seq + 1
         RETURNING last_seq",
    )
    .bind(day)
    .fetch_one(&mut *conn)
    .await?;
    let order_no = format_order_no(day, sequence);

    let order_id = Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO orders
         (id, order_no, trading_day, order_seq, order_type, table_no, status,
          subtotal_kobo, packaging_fee_kobo, total_kobo, user_id, terminal_id,
          created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, 'held', ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&order_id)
    .bind(&order_no)
    .bind(day)
    .bind(sequence)
    .bind(order_type.as_str())
    .bind(table_no)
    .bind(subtotal)
    .bind(fee)
    .bind(total.kobo())
    .bind(&actor.user.id)
    .bind(&actor.terminal_id)
    .bind(now)
    .bind(now)
    .execute(&mut *conn)
    .await?;

    insert_lines(conn, &order_id, &lines, now).await?;

    audit::record(
        &mut *conn,
        AuditEntry {
            user_id: Some(&actor.user.id),
            terminal_id: Some(&actor.terminal_id),
            action: "order.created",
            entity: Some("orders"),
            entity_id: Some(&order_id),
            details: Some(&order_no),
        },
    )
    .await?;
    audit::enqueue(
        &mut *conn,
        "orders",
        &order_id,
        "create",
        &serde_json::json!({
            "id": &order_id,
            "order_no": &order_no,
            "order_type": order_type.as_str(),
            "table_no": table_no,
            "status": "held",
            "subtotal_kobo": subtotal,
            "packaging_fee_kobo": fee,
            "total_kobo": total.kobo(),
            "user_id": &actor.user.id,
        }),
    )
    .await?;

    Ok(order_id)
}

/// Replaces the lines of a held order.
pub async fn update(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
    Json(request): Json<UpdateOrderRequest>,
) -> Result<Json<OrderDto>, ApiError> {
    let table_no = table_for(request.order_type, &request.table_no)?;
    let now = Utc::now().to_rfc3339();

    {
        let mut pooled = state.pool.acquire().await?;
        let conn = &mut *pooled;
        sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
        match update_inner(
            conn,
            &actor,
            &id,
            request.order_type,
            &table_no,
            &request.lines,
            &now,
        )
        .await
        {
            Ok(()) => {
                sqlx::query("COMMIT").execute(&mut *conn).await?;
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
                return Err(error);
            }
        }
    }

    let order = sales::load_order(&state.pool, &id).await?;
    state
        .events
        .send("order.updated", serde_json::json!({ "order_id": id }));
    Ok(Json(order))
}

async fn update_inner(
    conn: &mut SqliteConnection,
    actor: &AuthUser,
    id: &str,
    order_type: OrderType,
    table_no: &Option<String>,
    inputs: &[OrderLineInput],
    now: &str,
) -> Result<(), ApiError> {
    if load_status(conn, id).await? != OrderStatus::Held {
        return Err(ApiError::conflict(
            "order_immutable",
            "A paid or voided order cannot be changed. Void it instead.",
        ));
    }

    let (lines, subtotal) = sales::prepare_lines(conn, inputs, now).await?;
    let fee = packaging_fee(conn, order_type).await?;
    let total = order_total(
        &lines
            .iter()
            .map(|line| Money::from_kobo(line.line_total_kobo))
            .collect::<Vec<_>>(),
        Money::from_kobo(fee),
    )
    .ok_or_else(|| ApiError::bad_request("That order is too large to price."))?;

    sqlx::query(
        "UPDATE orders
         SET order_type = ?, table_no = ?, subtotal_kobo = ?, packaging_fee_kobo = ?,
             total_kobo = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(order_type.as_str())
    .bind(table_no)
    .bind(subtotal)
    .bind(fee)
    .bind(total.kobo())
    .bind(now)
    .bind(id)
    .execute(&mut *conn)
    .await?;

    sqlx::query("DELETE FROM order_lines WHERE order_id = ?")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    insert_lines(conn, id, &lines, now).await?;

    audit::record(
        &mut *conn,
        AuditEntry {
            user_id: Some(&actor.user.id),
            terminal_id: Some(&actor.terminal_id),
            action: "order.updated",
            entity: Some("orders"),
            entity_id: Some(id),
            details: None,
        },
    )
    .await?;
    audit::enqueue(
        &mut *conn,
        "orders",
        id,
        "update",
        &serde_json::json!({
            "id": id,
            "order_type": order_type.as_str(),
            "table_no": table_no,
            "subtotal_kobo": subtotal,
            "packaging_fee_kobo": fee,
            "total_kobo": total.kobo(),
        }),
    )
    .await?;

    Ok(())
}

/// Records payments, deducts stock and closes the order.
pub async fn pay(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
    Json(request): Json<PayOrderRequest>,
) -> Result<Json<PayOrderResponse>, ApiError> {
    let now = Utc::now().to_rfc3339();

    let change_kobo = {
        let mut pooled = state.pool.acquire().await?;
        let conn = &mut *pooled;
        sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
        match pay_inner(conn, &actor, &id, &request, &now).await {
            Ok(change_kobo) => {
                sqlx::query("COMMIT").execute(&mut *conn).await?;
                change_kobo
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
                return Err(error);
            }
        }
    };

    let order = sales::load_order(&state.pool, &id).await?;
    state.events.send(
        "order.paid",
        serde_json::json!({ "order_id": id, "order_no": order.order_no }),
    );
    state
        .events
        .send("stock.changed", serde_json::json!({ "order_id": id }));
    Ok(Json(PayOrderResponse { order, change_kobo }))
}

async fn pay_inner(
    conn: &mut SqliteConnection,
    actor: &AuthUser,
    id: &str,
    request: &PayOrderRequest,
    now: &str,
) -> Result<i64, ApiError> {
    let status = load_status(conn, id).await?;
    if status != OrderStatus::Held {
        return Err(ApiError::conflict(
            "order_closed",
            "That order is already closed.",
        ));
    }

    let total_kobo: i64 = sqlx::query_scalar("SELECT total_kobo FROM orders WHERE id = ?")
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
    let line_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM order_lines WHERE order_id = ?")
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
    if line_count == 0 {
        return Err(ApiError::bad_request("This order has no items."));
    }
    if request.payments.is_empty() {
        return Err(ApiError::bad_request("Record at least one payment."));
    }

    let mut tendered = Money::ZERO;
    for payment in &request.payments {
        if payment.amount_kobo <= 0 {
            return Err(ApiError::bad_request(
                "Every payment amount must be more than zero.",
            ));
        }
        tendered = tendered
            .checked_add(Money::from_kobo(payment.amount_kobo))
            .ok_or_else(|| ApiError::bad_request("That payment total is too large."))?;
    }
    if tendered.kobo() < total_kobo {
        let shortfall = Money::from_kobo(total_kobo - tendered.kobo());
        return Err(ApiError::bad_request(format!(
            "Payment is short by {shortfall}."
        )));
    }

    let requirements = sales::order_stock_requirements(conn, id).await?;
    for (item_id, (name, needed)) in &requirements {
        let balance: i64 = sqlx::query_scalar(
            "SELECT COALESCE((SELECT balance FROM stock_balances WHERE item_id = ?), 0)",
        )
        .bind(item_id)
        .fetch_one(&mut *conn)
        .await?;
        if balance < *needed {
            let message = if balance <= 0 {
                format!("{name} is sold out.")
            } else {
                format!("Only {balance} of {name} left in stock.")
            };
            return Err(ApiError::conflict("insufficient_stock", message));
        }
    }

    for (item_id, (_, needed)) in &requirements {
        sales::apply_stock_movement(
            &mut *conn,
            item_id,
            "sale",
            -needed,
            id,
            None,
            &actor.user.id,
            &actor.terminal_id,
            now,
        )
        .await?;
        audit::enqueue(
            &mut *conn,
            "stock_ledger",
            item_id,
            "create",
            &serde_json::json!({
                "item_id": item_id,
                "movement_type": "sale",
                "quantity": -needed,
                "ref_id": id,
            }),
        )
        .await?;
    }

    for payment in &request.payments {
        let payment_id = Uuid::now_v7().to_string();
        let reference = payment
            .reference
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        sqlx::query(
            "INSERT INTO payments
             (id, order_id, method, amount_kobo, reference, confirmed_by, confirmed_at,
              terminal_id, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&payment_id)
        .bind(id)
        .bind(payment.method.as_str())
        .bind(payment.amount_kobo)
        .bind(reference)
        .bind(&actor.user.id)
        .bind(now)
        .bind(&actor.terminal_id)
        .bind(now)
        .execute(&mut *conn)
        .await?;
        audit::enqueue(
            &mut *conn,
            "payments",
            &payment_id,
            "create",
            &serde_json::json!({
                "id": &payment_id,
                "order_id": id,
                "method": payment.method.as_str(),
                "amount_kobo": payment.amount_kobo,
                "reference": reference,
                "confirmed_by": &actor.user.id,
            }),
        )
        .await?;
    }

    sqlx::query("UPDATE orders SET status = 'paid', paid_at = ?, updated_at = ? WHERE id = ?")
        .bind(now)
        .bind(now)
        .bind(id)
        .execute(&mut *conn)
        .await?;

    audit::record(
        &mut *conn,
        AuditEntry {
            user_id: Some(&actor.user.id),
            terminal_id: Some(&actor.terminal_id),
            action: "order.paid",
            entity: Some("orders"),
            entity_id: Some(id),
            details: None,
        },
    )
    .await?;
    audit::enqueue(
        &mut *conn,
        "orders",
        id,
        "update",
        &serde_json::json!({ "id": id, "status": "paid", "paid_at": now }),
    )
    .await?;

    Ok(tendered.kobo() - total_kobo)
}

/// Voids a held order, or a paid one with a manager PIN and reason.
pub async fn void(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
    Json(request): Json<VoidOrderRequest>,
) -> Result<Json<OrderDto>, ApiError> {
    let order = sales::load_order(&state.pool, &id).await?;
    let reason = request.reason.trim().to_owned();

    let voided_by = match order.status {
        OrderStatus::Held => actor.user.id.clone(),
        OrderStatus::Voided => {
            return Err(ApiError::conflict(
                "order_closed",
                "That order has already been voided.",
            ))
        }
        OrderStatus::Paid => {
            if reason.is_empty() {
                return Err(ApiError::bad_request("Give a reason for the void."));
            }
            let pin = request
                .manager_pin
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    ApiError::unauthorized(
                        "manager_pin_required",
                        "Enter a manager PIN to void a paid order.",
                    )
                })?;
            verify_manager(&state, &actor, pin).await?.id
        }
    };

    let now = Utc::now().to_rfc3339();
    {
        let mut pooled = state.pool.acquire().await?;
        let conn = &mut *pooled;
        sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
        match void_inner(
            conn,
            &actor,
            &id,
            order.status,
            &voided_by,
            &reason,
            request.return_to_stock,
            &now,
        )
        .await
        {
            Ok(()) => {
                sqlx::query("COMMIT").execute(&mut *conn).await?;
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
                return Err(error);
            }
        }
    }

    let order = sales::load_order(&state.pool, &id).await?;
    state.events.send(
        "order.voided",
        serde_json::json!({ "order_id": id, "order_no": order.order_no }),
    );
    state
        .events
        .send("stock.changed", serde_json::json!({ "order_id": id }));
    Ok(Json(order))
}

async fn verify_manager(
    state: &AppState,
    actor: &AuthUser,
    pin: &str,
) -> Result<pos_types::UserSummary, ApiError> {
    if auth::is_locked_out(&state.pool, &actor.terminal_id).await? {
        return Err(ApiError::locked(
            "Too many wrong PINs on this terminal. Wait a moment and try again.",
            auth::LOCKOUT_MINUTES as u64 * 60,
        ));
    }
    match auth::authenticate(&state.pool, pin).await? {
        Some(user) if user.role.can_void() => {
            auth::clear_failures(&state.pool, &actor.terminal_id).await?;
            Ok(user)
        }
        Some(_) => {
            auth::record_attempt(&state.pool, &actor.terminal_id, false).await?;
            Err(ApiError::forbidden(
                "A manager or owner PIN is needed for this action.",
            ))
        }
        None => {
            auth::record_attempt(&state.pool, &actor.terminal_id, false).await?;
            Err(ApiError::unauthorized(
                "invalid_pin",
                "That PIN is not correct.",
            ))
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn void_inner(
    conn: &mut SqliteConnection,
    actor: &AuthUser,
    id: &str,
    expected_status: OrderStatus,
    voided_by: &str,
    reason: &str,
    return_to_stock: bool,
    now: &str,
) -> Result<(), ApiError> {
    let status = load_status(conn, id).await?;
    if status != expected_status || status == OrderStatus::Voided {
        return Err(ApiError::conflict(
            "order_changed",
            "That order changed while you were voiding it. Reopen it and try again.",
        ));
    }

    if status == OrderStatus::Paid && return_to_stock {
        let requirements = sales::order_stock_requirements(conn, id).await?;
        for (item_id, (_, quantity)) in &requirements {
            sales::apply_stock_movement(
                &mut *conn,
                item_id,
                "void_return",
                *quantity,
                id,
                Some(reason),
                &actor.user.id,
                &actor.terminal_id,
                now,
            )
            .await?;
            audit::enqueue(
                &mut *conn,
                "stock_ledger",
                item_id,
                "create",
                &serde_json::json!({
                    "item_id": item_id,
                    "movement_type": "void_return",
                    "quantity": quantity,
                    "ref_id": id,
                }),
            )
            .await?;
        }
    }

    let reason_text = if reason.is_empty() {
        "discarded"
    } else {
        reason
    };
    sqlx::query(
        "UPDATE orders
         SET status = 'voided', voided_at = ?, voided_by = ?, void_reason = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(now)
    .bind(voided_by)
    .bind(reason_text)
    .bind(now)
    .bind(id)
    .execute(&mut *conn)
    .await?;

    audit::record(
        &mut *conn,
        AuditEntry {
            user_id: Some(&actor.user.id),
            terminal_id: Some(&actor.terminal_id),
            action: "order.voided",
            entity: Some("orders"),
            entity_id: Some(id),
            details: Some(reason_text),
        },
    )
    .await?;
    audit::enqueue(
        &mut *conn,
        "orders",
        id,
        "update",
        &serde_json::json!({
            "id": id,
            "status": "voided",
            "voided_at": now,
            "voided_by": voided_by,
            "void_reason": reason_text,
            "return_to_stock": return_to_stock,
        }),
    )
    .await?;

    Ok(())
}

/// One order, for the receipt or a resumed sale.
pub async fn get(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<OrderDto>, ApiError> {
    let _ = actor;
    Ok(Json(sales::load_order(&state.pool, &id).await?))
}

/// Held orders waiting to be resumed.
pub async fn held(
    State(state): State<AppState>,
    actor: AuthUser,
) -> Result<Json<Vec<OrderDto>>, ApiError> {
    let _ = actor;
    let ids: Vec<String> =
        sqlx::query_scalar("SELECT id FROM orders WHERE status = 'held' ORDER BY created_at")
            .fetch_all(&state.pool)
            .await?;

    let mut orders = Vec::with_capacity(ids.len());
    for id in ids {
        orders.push(sales::load_order(&state.pool, &id).await?);
    }
    Ok(Json(orders))
}
