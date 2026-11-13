//! Turns an order into the exact text stored with a print job.

use chrono::{DateTime, Duration, Utc};
use pos_core::Money;
use pos_print::{
    render_receipt, render_slip, PaperWidth, Receipt, ReceiptItem, ReceiptPayment, Slip,
};
use pos_types::{OrderDto, OrderStatus, OrderType, PaymentMethod};
use sqlx::{Row, SqlitePool};

use crate::error::ApiError;

/// The business details printed at the top of every receipt.
pub struct Business {
    pub name: String,
    pub address: String,
    pub phone: String,
    pub footer: String,
}

pub async fn business(pool: &SqlitePool) -> Result<Business, ApiError> {
    let row = sqlx::query(
        "SELECT business_name, business_address, business_phone, receipt_footer
         FROM settings WHERE id = 1",
    )
    .fetch_one(pool)
    .await?;
    Ok(Business {
        name: row.get("business_name"),
        address: row.get("business_address"),
        phone: row.get("business_phone"),
        footer: row.get("receipt_footer"),
    })
}

fn wat_time(value: &str) -> String {
    DateTime::parse_from_rfc3339(value)
        .map(|at| {
            (at.with_timezone(&Utc) + Duration::hours(1))
                .format("%d/%m/%Y %H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| value.to_owned())
}

fn order_type_label(order_type: OrderType) -> &'static str {
    match order_type {
        OrderType::DineIn => "Dine-in",
        OrderType::Takeaway => "Takeaway",
    }
}

fn payment_label(method: PaymentMethod) -> &'static str {
    match method {
        PaymentMethod::Cash => "Cash",
        PaymentMethod::Transfer => "Transfer",
        PaymentMethod::Card => "Card",
    }
}

fn component_detail(name: &str, quantity: i64, unit: pos_types::UnitType) -> String {
    let suffix = if quantity == 1 { "" } else { "s" };
    format!("{quantity} {}{suffix} {name}", unit.label())
}

fn line_details(line: &pos_types::OrderLineDto) -> Vec<String> {
    if line.kind != "pack" {
        return Vec::new();
    }
    line.components
        .iter()
        .map(|component| {
            let per_pack = if line.quantity > 0 {
                component.quantity / line.quantity
            } else {
                component.quantity
            };
            component_detail(&component.item_name, per_pack, component.unit_type)
        })
        .collect()
}

fn paid_change(order: &OrderDto) -> Option<Money> {
    if order.status != OrderStatus::Paid {
        return None;
    }
    let tendered: i64 = order
        .payments
        .iter()
        .map(|payment| payment.amount_kobo)
        .sum();
    let change = tendered - order.total_kobo;
    (change > 0).then(|| Money::from_kobo(change))
}

/// Builds the customer receipt text for an order.
pub fn build_receipt(
    order: &OrderDto,
    business: &Business,
    width: PaperWidth,
    duplicate: bool,
) -> String {
    let items = order
        .lines
        .iter()
        .map(|line| ReceiptItem {
            name: line.name.clone(),
            quantity: line.quantity,
            unit_price: Money::from_kobo(line.unit_price_kobo),
            total: Money::from_kobo(line.line_total_kobo),
            details: line_details(line),
        })
        .collect();
    let payments = order
        .payments
        .iter()
        .map(|payment| ReceiptPayment {
            method: payment_label(payment.method).to_owned(),
            amount: Money::from_kobo(payment.amount_kobo),
        })
        .collect();

    let receipt = Receipt {
        business_name: business.name.clone(),
        business_address: business.address.clone(),
        business_phone: business.phone.clone(),
        footer: business.footer.clone(),
        order_no: order.order_no.clone(),
        order_type: order_type_label(order.order_type).to_owned(),
        table_no: order.table_no.clone(),
        cashier: order.user_name.clone(),
        created_at: wat_time(order.paid_at.as_deref().unwrap_or(&order.created_at)),
        items,
        subtotal: Money::from_kobo(order.subtotal_kobo),
        packaging_fee: Money::from_kobo(order.packaging_fee_kobo),
        total: Money::from_kobo(order.total_kobo),
        payments,
        change: paid_change(order),
        duplicate,
    };
    render_receipt(&receipt, width)
}

/// Builds the serving slip, which lists quantities and pack contents only.
pub fn build_slip(order: &OrderDto, business: &Business, width: PaperWidth) -> String {
    let slip = Slip {
        business_name: business.name.clone(),
        order_no: order.order_no.clone(),
        order_type: order_type_label(order.order_type).to_owned(),
        table_no: order.table_no.clone(),
        created_at: wat_time(&order.created_at),
        lines: order
            .lines
            .iter()
            .map(|line| (line.name.clone(), line.quantity, line_details(line)))
            .collect(),
    };
    render_slip(&slip, width)
}
