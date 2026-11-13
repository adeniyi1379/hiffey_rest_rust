//! End-of-day reports and CSV exports for the owner.

use std::collections::HashMap;

use axum::{
    body::Body,
    extract::{Query, State},
    http::header,
    response::Response,
    Json,
};
use chrono::{Duration, NaiveDate, Utc};
use pos_types::{DailyReport, PaymentBreakdownDto, PaymentMethod, ReportItemDto, Role, UnitType};
use serde::Deserialize;
use sqlx::Row;

use crate::{auth::AuthUser, error::ApiError, AppState};

#[derive(Debug, Deserialize)]
pub struct DailyQuery {
    /// `YYYY-MM-DD`; defaults to today in the shop's timezone.
    pub date: Option<String>,
}

fn today_wat() -> String {
    (Utc::now() + Duration::hours(1))
        .format("%Y-%m-%d")
        .to_string()
}

/// Validates the requested day and returns it in both display and stored forms.
fn day_pair(input: Option<&str>) -> Result<(String, String), ApiError> {
    let text = input.map(str::trim).filter(|value| !value.is_empty());
    let text = match text {
        Some(text) => text.to_owned(),
        None => today_wat(),
    };
    let date = NaiveDate::parse_from_str(&text, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("Use a date in the form YYYY-MM-DD."))?;
    Ok((
        date.format("%Y-%m-%d").to_string(),
        date.format("%Y%m%d").to_string(),
    ))
}

fn parse_unit(value: &str) -> Result<UnitType, ApiError> {
    value
        .parse::<UnitType>()
        .map_err(|()| ApiError::internal(format!("unknown unit type in database: {value}")))
}

/// Aggregates one trading day: sales, payments and the item mix.
pub async fn daily(
    State(state): State<AppState>,
    actor: AuthUser,
    Query(query): Query<DailyQuery>,
) -> Result<Json<DailyReport>, ApiError> {
    crate::auth::require(&actor, &[Role::Owner, Role::Manager])?;
    let (date, day) = day_pair(query.date.as_deref())?;

    let stats = sqlx::query(
        "SELECT
            COALESCE(SUM(CASE WHEN status = 'paid' THEN 1 ELSE 0 END), 0) AS paid_orders,
            COALESCE(SUM(CASE WHEN status = 'held' THEN 1 ELSE 0 END), 0) AS held_orders,
            COALESCE(SUM(CASE WHEN status = 'voided' THEN 1 ELSE 0 END), 0) AS voided_orders,
            COALESCE(SUM(CASE WHEN status = 'paid' AND order_type = 'dine_in' THEN 1 ELSE 0 END), 0)
                AS dine_in_orders,
            COALESCE(SUM(CASE WHEN status = 'paid' AND order_type = 'takeaway' THEN 1 ELSE 0 END), 0)
                AS takeaway_orders,
            COALESCE(SUM(CASE WHEN status = 'paid' THEN subtotal_kobo ELSE 0 END), 0) AS subtotal_kobo,
            COALESCE(SUM(CASE WHEN status = 'paid' THEN packaging_fee_kobo ELSE 0 END), 0)
                AS packaging_kobo,
            COALESCE(SUM(CASE WHEN status = 'paid' THEN total_kobo ELSE 0 END), 0) AS total_kobo,
            COALESCE(SUM(CASE WHEN status = 'voided' THEN total_kobo ELSE 0 END), 0)
                AS voided_total_kobo
         FROM orders WHERE trading_day = ?",
    )
    .bind(&day)
    .fetch_one(&state.pool)
    .await?;

    let paid_orders: i64 = stats.get("paid_orders");
    let total_kobo: i64 = stats.get("total_kobo");
    let average_kobo = if paid_orders == 0 {
        0
    } else {
        total_kobo / paid_orders
    };

    let tendered_kobo: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(p.amount_kobo), 0)
         FROM payments p JOIN orders o ON o.id = p.order_id
         WHERE o.trading_day = ? AND o.status = 'paid'",
    )
    .bind(&day)
    .fetch_one(&state.pool)
    .await?;
    let change_kobo = (tendered_kobo - total_kobo).max(0);

    let payment_rows = sqlx::query(
        "SELECT p.method, COUNT(DISTINCT p.order_id) AS orders,
                COALESCE(SUM(p.amount_kobo), 0) AS amount_kobo
         FROM payments p JOIN orders o ON o.id = p.order_id
         WHERE o.trading_day = ? AND o.status = 'paid'
         GROUP BY p.method",
    )
    .bind(&day)
    .fetch_all(&state.pool)
    .await?;
    let mut by_method = HashMap::new();
    for row in &payment_rows {
        by_method.insert(
            row.get::<String, _>("method"),
            (
                row.get::<i64, _>("orders"),
                row.get::<i64, _>("amount_kobo"),
            ),
        );
    }
    let payments = [
        PaymentMethod::Cash,
        PaymentMethod::Transfer,
        PaymentMethod::Card,
    ]
    .into_iter()
    .map(|method| {
        let (orders, amount_kobo) = by_method.get(method.as_str()).copied().unwrap_or((0, 0));
        PaymentBreakdownDto {
            method,
            orders,
            amount_kobo,
        }
    })
    .collect();

    let item_rows = sqlx::query(
        "SELECT c.item_id, c.item_name, c.unit_type,
                SUM(c.quantity) AS quantity,
                SUM(c.quantity * c.unit_price_kobo) AS revenue_kobo
         FROM order_line_components c
         JOIN order_lines l ON l.id = c.order_line_id
         JOIN orders o ON o.id = l.order_id
         WHERE o.trading_day = ? AND o.status = 'paid'
         GROUP BY c.item_id, c.item_name, c.unit_type
         ORDER BY revenue_kobo DESC, c.item_name COLLATE NOCASE",
    )
    .bind(&day)
    .fetch_all(&state.pool)
    .await?;

    let mut items = Vec::with_capacity(item_rows.len());
    for row in &item_rows {
        let unit_text: String = row.get("unit_type");
        items.push(ReportItemDto {
            item_id: row.get("item_id"),
            item_name: row.get("item_name"),
            unit_type: parse_unit(&unit_text)?,
            quantity: row.get("quantity"),
            revenue_kobo: row.get("revenue_kobo"),
        });
    }

    Ok(Json(DailyReport {
        date,
        paid_orders,
        dine_in_orders: stats.get("dine_in_orders"),
        takeaway_orders: stats.get("takeaway_orders"),
        held_orders: stats.get("held_orders"),
        voided_orders: stats.get("voided_orders"),
        subtotal_kobo: stats.get("subtotal_kobo"),
        packaging_kobo: stats.get("packaging_kobo"),
        total_kobo,
        average_kobo,
        tendered_kobo,
        change_kobo,
        voided_total_kobo: stats.get("voided_total_kobo"),
        payments,
        items,
    }))
}

fn naira(kobo: i64) -> String {
    let sign = if kobo < 0 { "-" } else { "" };
    format!("{sign}{}.{:02}", kobo.abs() / 100, kobo.abs() % 100)
}

fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

fn csv_row(fields: &[String]) -> String {
    let line = fields
        .iter()
        .map(|field| csv_field(field))
        .collect::<Vec<_>>()
        .join(",");
    format!("{line}\r\n")
}

fn csv_response(filename: &str, csv: String) -> Result<Response, ApiError> {
    Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, "text/csv; charset=utf-8")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )
        .body(Body::from(csv))
        .map_err(|error| ApiError::internal(format!("csv response: {error}")))
}

/// One row per order for the day, with the payment split and void reason.
pub async fn sales_csv(
    State(state): State<AppState>,
    actor: AuthUser,
    Query(query): Query<DailyQuery>,
) -> Result<Response, ApiError> {
    crate::auth::require(&actor, &[Role::Owner, Role::Manager])?;
    let (date, day) = day_pair(query.date.as_deref())?;

    let rows = sqlx::query(
        "SELECT o.order_no, o.trading_day, o.status, o.order_type, o.table_no,
                u.name AS cashier, o.created_at, o.paid_at,
                o.subtotal_kobo, o.packaging_fee_kobo, o.total_kobo, o.void_reason,
                COALESCE(SUM(CASE WHEN p.method = 'cash' THEN p.amount_kobo ELSE 0 END), 0)
                    AS cash_kobo,
                COALESCE(SUM(CASE WHEN p.method = 'transfer' THEN p.amount_kobo ELSE 0 END), 0)
                    AS transfer_kobo,
                COALESCE(SUM(CASE WHEN p.method = 'card' THEN p.amount_kobo ELSE 0 END), 0)
                    AS card_kobo
         FROM orders o
         JOIN users u ON u.id = o.user_id
         LEFT JOIN payments p ON p.order_id = o.id
         WHERE o.trading_day = ?
         GROUP BY o.id
         ORDER BY o.created_at",
    )
    .bind(&day)
    .fetch_all(&state.pool)
    .await?;

    let mut csv = String::new();
    csv.push_str(
        "order_no,trading_day,status,order_type,table,cashier,created_at,paid_at,\
         subtotal,packaging,total,cash,transfer,card,void_reason\r\n",
    );
    for row in &rows {
        let paid_at: Option<String> = row.get("paid_at");
        let table: Option<String> = row.get("table_no");
        let reason: Option<String> = row.get("void_reason");
        csv.push_str(&csv_row(&[
            row.get("order_no"),
            row.get("trading_day"),
            row.get("status"),
            row.get("order_type"),
            table.unwrap_or_default(),
            row.get("cashier"),
            row.get("created_at"),
            paid_at.unwrap_or_default(),
            naira(row.get("subtotal_kobo")),
            naira(row.get("packaging_fee_kobo")),
            naira(row.get("total_kobo")),
            naira(row.get("cash_kobo")),
            naira(row.get("transfer_kobo")),
            naira(row.get("card_kobo")),
            reason.unwrap_or_default(),
        ]));
    }

    csv_response(&format!("hiffey-sales-{date}.csv"), csv)
}

/// Every stock movement recorded on the day, for the stock book.
pub async fn stock_csv(
    State(state): State<AppState>,
    actor: AuthUser,
    Query(query): Query<DailyQuery>,
) -> Result<Response, ApiError> {
    crate::auth::require(&actor, &[Role::Owner, Role::Manager])?;
    let (date, _day) = day_pair(query.date.as_deref())?;

    let rows = sqlx::query(
        "SELECT l.created_at, i.name AS item_name, l.movement_type, l.quantity,
                l.reason, u.name AS user_name
         FROM stock_ledger l
         JOIN items i ON i.id = l.item_id
         LEFT JOIN users u ON u.id = l.user_id
         WHERE date(l.created_at, '+1 hour') = ?
         ORDER BY l.created_at",
    )
    .bind(&date)
    .fetch_all(&state.pool)
    .await?;

    let mut csv = String::new();
    csv.push_str("created_at,item,movement,quantity,reason,worker\r\n");
    for row in &rows {
        let reason: Option<String> = row.get("reason");
        let user: Option<String> = row.get("user_name");
        csv.push_str(&csv_row(&[
            row.get("created_at"),
            row.get("item_name"),
            row.get("movement_type"),
            row.get::<i64, _>("quantity").to_string(),
            reason.unwrap_or_default(),
            user.unwrap_or_default(),
        ]));
    }

    csv_response(&format!("hiffey-stock-{date}.csv"), csv)
}

#[cfg(test)]
mod tests {
    use super::{csv_field, day_pair, naira};

    #[test]
    fn csv_fields_are_quoted_when_needed() {
        assert_eq!(csv_field("Jollof Rice"), "Jollof Rice");
        assert_eq!(csv_field("Rice, beans"), "\"Rice, beans\"");
        assert_eq!(csv_field("He said \"no\""), "\"He said \"\"no\"\"\"");
        assert_eq!(csv_field("two\nlines"), "\"two\nlines\"");
    }

    #[test]
    fn naira_csv_values_keep_kobo() {
        assert_eq!(naira(0), "0.00");
        assert_eq!(naira(70_000), "700.00");
        assert_eq!(naira(125_005), "1250.05");
        assert_eq!(naira(-2_500), "-25.00");
    }

    #[test]
    fn days_must_be_real_dates() {
        assert_eq!(
            day_pair(Some("2026-11-12")).unwrap(),
            ("2026-11-12".to_owned(), "20261112".to_owned())
        );
        assert!(day_pair(Some("2026-13-01")).is_err());
        assert!(day_pair(Some("12/11/2026")).is_err());
    }
}
