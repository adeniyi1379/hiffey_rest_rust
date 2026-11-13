//! Order preparation and loading, shared by the order handlers.

use std::collections::HashMap;

use pos_core::{line_total, pack_price, Money};
use pos_types::{
    OrderDto, OrderLineComponentDto, OrderLineDto, OrderLineInput, OrderStatus, OrderType,
    PaymentDto, PaymentMethod, UnitType,
};
use sqlx::{Row, SqliteConnection, SqlitePool};
use uuid::Uuid;

use crate::error::ApiError;

/// A line ready to be written, with its stock-bearing components snapshotted.
#[derive(Debug, Clone)]
pub struct PreparedComponent {
    pub item_id: String,
    pub item_name: String,
    pub unit_type: UnitType,
    pub quantity: i64,
    pub unit_price_kobo: i64,
}

#[derive(Debug, Clone)]
pub struct PreparedLine {
    pub kind: &'static str,
    pub item_id: Option<String>,
    pub pack_id: Option<String>,
    pub name: String,
    pub quantity: i64,
    pub unit_price_kobo: i64,
    pub line_total_kobo: i64,
    pub components: Vec<PreparedComponent>,
}

fn parse_unit(value: &str) -> Result<UnitType, ApiError> {
    value
        .parse::<UnitType>()
        .map_err(|()| ApiError::internal(format!("unknown unit type in database: {value}")))
}

fn price_sql() -> &'static str {
    "COALESCE(
        (SELECT p.price_kobo FROM item_prices p
         WHERE p.item_id = i.id AND p.effective_from <= ?
         ORDER BY p.effective_from DESC LIMIT 1),
        i.price_per_unit_kobo
    )"
}

/// Validates the requested lines against the current menu and prices them.
///
/// This runs inside the sale transaction so the price charged is the price that
/// was current when the order was taken.
pub async fn prepare_lines(
    conn: &mut SqliteConnection,
    inputs: &[OrderLineInput],
    at: &str,
) -> Result<(Vec<PreparedLine>, i64), ApiError> {
    if inputs.is_empty() {
        return Err(ApiError::bad_request("Add at least one item to the order."));
    }

    let mut prepared = Vec::with_capacity(inputs.len());
    let mut subtotal = Money::ZERO;

    for input in inputs {
        let line = match input {
            OrderLineInput::Item { item_id, quantity } => {
                prepare_item_line(conn, item_id, *quantity, at).await?
            }
            OrderLineInput::Pack { pack_id, quantity } => {
                prepare_pack_line(conn, pack_id, *quantity, at).await?
            }
        };
        subtotal = subtotal
            .checked_add(Money::from_kobo(line.line_total_kobo))
            .ok_or_else(|| ApiError::bad_request("That order is too large to price."))?;
        prepared.push(line);
    }

    Ok((prepared, subtotal.kobo()))
}

async fn prepare_item_line(
    conn: &mut SqliteConnection,
    item_id: &str,
    quantity: i64,
    at: &str,
) -> Result<PreparedLine, ApiError> {
    if quantity <= 0 {
        return Err(ApiError::bad_request("Every quantity must be at least 1."));
    }

    let sql = format!(
        "SELECT i.name, i.unit_type, i.active, {} AS current_price
         FROM items i WHERE i.id = ?",
        price_sql()
    );
    let row = sqlx::query(&sql)
        .bind(at)
        .bind(item_id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| ApiError::bad_request("One of the items is no longer on the menu."))?;

    if row.get::<i64, _>("active") == 0 {
        return Err(ApiError::bad_request(format!(
            "{} is not available right now.",
            row.get::<String, _>("name")
        )));
    }

    let unit_type: String = row.get("unit_type");
    let unit_type = parse_unit(&unit_type)?;
    let price = Money::from_kobo(row.get("current_price"));
    let total = line_total(price, quantity)
        .ok_or_else(|| ApiError::bad_request("That quantity is not valid."))?;
    let name: String = row.get("name");

    Ok(PreparedLine {
        kind: "item",
        item_id: Some(item_id.to_owned()),
        pack_id: None,
        name: name.clone(),
        quantity,
        unit_price_kobo: price.kobo(),
        line_total_kobo: total.kobo(),
        components: vec![PreparedComponent {
            item_id: item_id.to_owned(),
            item_name: name,
            unit_type,
            quantity,
            unit_price_kobo: price.kobo(),
        }],
    })
}

async fn prepare_pack_line(
    conn: &mut SqliteConnection,
    pack_id: &str,
    quantity: i64,
    at: &str,
) -> Result<PreparedLine, ApiError> {
    if quantity <= 0 {
        return Err(ApiError::bad_request("Every quantity must be at least 1."));
    }

    let pack =
        sqlx::query("SELECT name, pricing_mode, fixed_price_kobo, active FROM packs WHERE id = ?")
            .bind(pack_id)
            .fetch_optional(&mut *conn)
            .await?
            .ok_or_else(|| ApiError::bad_request("One of the packs is no longer on the menu."))?;

    let pack_name: String = pack.get("name");
    if pack.get::<i64, _>("active") == 0 {
        return Err(ApiError::bad_request(format!(
            "The {pack_name} pack is not available right now."
        )));
    }

    let sql = format!(
        "SELECT pc.item_id, pc.quantity, i.name, i.unit_type, i.active, {} AS current_price
         FROM pack_components pc
         JOIN items i ON i.id = pc.item_id
         WHERE pc.pack_id = ?
         ORDER BY i.name COLLATE NOCASE",
        price_sql()
    );
    let rows = sqlx::query(&sql)
        .bind(at)
        .bind(pack_id)
        .fetch_all(&mut *conn)
        .await?;
    if rows.is_empty() {
        return Err(ApiError::bad_request(format!(
            "The {pack_name} pack has no items."
        )));
    }

    let mut components = Vec::with_capacity(rows.len());
    let mut price_pairs = Vec::with_capacity(rows.len());
    for row in &rows {
        let component_name: String = row.get("name");
        if row.get::<i64, _>("active") == 0 {
            return Err(ApiError::bad_request(format!(
                "{component_name} inside {pack_name} is not available right now."
            )));
        }
        let unit_type: String = row.get("unit_type");
        let unit_type = parse_unit(&unit_type)?;
        let component_quantity: i64 = row.get("quantity");
        let unit_price = Money::from_kobo(row.get("current_price"));
        let packs_sold = component_quantity
            .checked_mul(quantity)
            .ok_or_else(|| ApiError::bad_request("That quantity is not valid."))?;

        price_pairs.push((unit_price, component_quantity));
        components.push(PreparedComponent {
            item_id: row.get("item_id"),
            item_name: component_name,
            unit_type,
            quantity: packs_sold,
            unit_price_kobo: unit_price.kobo(),
        });
    }

    let mode_fixed = pack.get::<String, _>("pricing_mode") == "fixed";
    let fixed_price: Option<i64> = pack.get("fixed_price_kobo");
    let pack_unit_price = pack_price(mode_fixed, fixed_price.map(Money::from_kobo), &price_pairs)
        .ok_or_else(|| ApiError::bad_request("That pack cannot be priced."))?;
    let total = line_total(pack_unit_price, quantity)
        .ok_or_else(|| ApiError::bad_request("That quantity is not valid."))?;

    Ok(PreparedLine {
        kind: "pack",
        item_id: None,
        pack_id: Some(pack_id.to_owned()),
        name: pack_name,
        quantity,
        unit_price_kobo: pack_unit_price.kobo(),
        line_total_kobo: total.kobo(),
        components,
    })
}

/// The quantity of each item an order needs from stock, with display names.
pub async fn order_stock_requirements(
    conn: &mut SqliteConnection,
    order_id: &str,
) -> Result<HashMap<String, (String, i64)>, ApiError> {
    let rows = sqlx::query(
        "SELECT c.item_id, c.item_name, SUM(c.quantity) AS quantity
         FROM order_line_components c
         JOIN order_lines l ON l.id = c.order_line_id
         WHERE l.order_id = ?
         GROUP BY c.item_id, c.item_name",
    )
    .bind(order_id)
    .fetch_all(&mut *conn)
    .await?;

    let mut requirements = HashMap::with_capacity(rows.len());
    for row in &rows {
        requirements.insert(
            row.get("item_id"),
            (row.get("item_name"), row.get("quantity")),
        );
    }
    Ok(requirements)
}

/// Writes one append-only ledger row and keeps the cached balance in step.
#[allow(clippy::too_many_arguments)]
pub async fn apply_stock_movement(
    conn: &mut SqliteConnection,
    item_id: &str,
    movement_type: &str,
    quantity: i64,
    ref_id: &str,
    reason: Option<&str>,
    user_id: &str,
    terminal_id: &str,
    now: &str,
) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO stock_ledger
         (id, item_id, movement_type, quantity, ref_id, reason, user_id, terminal_id, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(item_id)
    .bind(movement_type)
    .bind(quantity)
    .bind(ref_id)
    .bind(reason)
    .bind(user_id)
    .bind(terminal_id)
    .bind(now)
    .execute(&mut *conn)
    .await?;

    sqlx::query(
        "INSERT INTO stock_balances (item_id, balance, updated_at)
         VALUES (?, ?, ?)
         ON CONFLICT (item_id) DO UPDATE SET
            balance = stock_balances.balance + excluded.balance,
            updated_at = excluded.updated_at",
    )
    .bind(item_id)
    .bind(quantity)
    .bind(now)
    .execute(&mut *conn)
    .await?;

    Ok(())
}

/// Loads a complete order for the UI.
pub async fn load_order(pool: &SqlitePool, id: &str) -> Result<OrderDto, ApiError> {
    let row = sqlx::query(
        "SELECT o.*, u.name AS user_name
         FROM orders o JOIN users u ON u.id = o.user_id
         WHERE o.id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::not_found("Order not found."))?;

    let status_text: String = row.get("status");
    let status = status_text
        .parse::<OrderStatus>()
        .map_err(|()| ApiError::internal(format!("unknown order status: {status_text}")))?;
    let type_text: String = row.get("order_type");
    let order_type = type_text
        .parse::<OrderType>()
        .map_err(|()| ApiError::internal(format!("unknown order type: {type_text}")))?;

    let line_rows = sqlx::query(
        "SELECT id, kind, item_id, pack_id, name, quantity, unit_price_kobo, line_total_kobo
         FROM order_lines WHERE order_id = ? ORDER BY line_no",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;

    let component_rows = sqlx::query(
        "SELECT c.order_line_id, c.item_id, c.item_name, c.unit_type, c.quantity, c.unit_price_kobo
         FROM order_line_components c
         JOIN order_lines l ON l.id = c.order_line_id
         WHERE l.order_id = ?",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;

    let mut components: HashMap<String, Vec<OrderLineComponentDto>> = HashMap::new();
    for component in &component_rows {
        let unit_text: String = component.get("unit_type");
        let unit_type = parse_unit(&unit_text)?;
        components
            .entry(component.get("order_line_id"))
            .or_default()
            .push(OrderLineComponentDto {
                item_id: component.get("item_id"),
                item_name: component.get("item_name"),
                unit_type,
                quantity: component.get("quantity"),
                unit_price_kobo: component.get("unit_price_kobo"),
            });
    }

    let lines: Vec<OrderLineDto> = line_rows
        .iter()
        .map(|line| {
            let line_id: String = line.get("id");
            OrderLineDto {
                id: line_id.clone(),
                kind: line.get("kind"),
                item_id: line.get("item_id"),
                pack_id: line.get("pack_id"),
                name: line.get("name"),
                quantity: line.get("quantity"),
                unit_price_kobo: line.get("unit_price_kobo"),
                line_total_kobo: line.get("line_total_kobo"),
                components: components.remove(&line_id).unwrap_or_default(),
            }
        })
        .collect();

    let payment_rows = sqlx::query(
        "SELECT p.id, p.method, p.amount_kobo, p.reference, p.confirmed_by,
                u.name AS confirmed_by_name, p.confirmed_at
         FROM payments p JOIN users u ON u.id = p.confirmed_by
         WHERE p.order_id = ? ORDER BY p.confirmed_at",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;

    let mut payments = Vec::with_capacity(payment_rows.len());
    for payment in &payment_rows {
        let method_text: String = payment.get("method");
        let method = method_text
            .parse::<PaymentMethod>()
            .map_err(|()| ApiError::internal(format!("unknown payment method: {method_text}")))?;
        payments.push(PaymentDto {
            id: payment.get("id"),
            method,
            amount_kobo: payment.get("amount_kobo"),
            reference: payment.get("reference"),
            confirmed_by: payment.get("confirmed_by"),
            confirmed_by_name: payment.get("confirmed_by_name"),
            confirmed_at: payment.get("confirmed_at"),
        });
    }

    Ok(OrderDto {
        id: row.get("id"),
        order_no: row.get("order_no"),
        trading_day: row.get("trading_day"),
        order_type,
        table_no: row.get("table_no"),
        status,
        subtotal_kobo: row.get("subtotal_kobo"),
        packaging_fee_kobo: row.get("packaging_fee_kobo"),
        total_kobo: row.get("total_kobo"),
        user_id: row.get("user_id"),
        user_name: row.get("user_name"),
        terminal_id: row.get("terminal_id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        paid_at: row.get("paid_at"),
        voided_at: row.get("voided_at"),
        void_reason: row.get("void_reason"),
        lines,
        payments,
    })
}
