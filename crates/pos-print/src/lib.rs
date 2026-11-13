//! Receipt and serving-slip layout, plus ESC/POS framing.
//!
//! The layout is plain text measured in printer columns so it can be previewed
//! on screen, stored with the print job, and sent to the Windows spooler. The
//! ESC/POS wrapper only adds the init and cut commands around that text.

use pos_core::Money;

/// Receipt paper width supported by the thermal printers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaperWidth {
    /// 58 mm printers (about 32 characters per line).
    Mm58,
    /// 80 mm printers (about 48 characters per line).
    Mm80,
}

impl PaperWidth {
    #[must_use]
    pub const fn columns(self) -> usize {
        match self {
            Self::Mm58 => 32,
            Self::Mm80 => 48,
        }
    }

    /// Parses `mm58` / `mm80`, defaulting to 80 mm.
    #[must_use]
    pub fn parse(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("mm58" | "58") => Self::Mm58,
            _ => Self::Mm80,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mm58 => "mm58",
            Self::Mm80 => "mm80",
        }
    }
}

/// The printer configured on this terminal. Stored locally on each PC, not in the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrinterConfig {
    pub name: String,
    pub width: PaperWidth,
}

/// One sold line: a custom plate or a whole pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptItem {
    pub name: String,
    pub quantity: i64,
    pub unit_price: Money,
    pub total: Money,
    /// Pack contents, one short line each (already worded for the paper).
    pub details: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptPayment {
    pub method: String,
    pub amount: Money,
}

/// Everything printed on a customer receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    pub business_name: String,
    pub business_address: String,
    pub business_phone: String,
    pub footer: String,
    pub order_no: String,
    pub order_type: String,
    pub table_no: Option<String>,
    pub cashier: String,
    pub created_at: String,
    pub items: Vec<ReceiptItem>,
    pub subtotal: Money,
    pub packaging_fee: Money,
    pub total: Money,
    pub payments: Vec<ReceiptPayment>,
    pub change: Option<Money>,
    pub duplicate: bool,
}

/// The serving slip handed to the kitchen or the person giving out food.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slip {
    pub business_name: String,
    pub order_no: String,
    pub order_type: String,
    pub table_no: Option<String>,
    pub created_at: String,
    /// (line name, quantity, pack contents)
    pub lines: Vec<(String, i64, Vec<String>)>,
}

fn line(out: &mut String, text: &str) {
    out.push_str(text);
    out.push('\n');
}

fn rule(out: &mut String, columns: usize, glyph: char) {
    line(out, &glyph.to_string().repeat(columns));
}

fn wrap(text: &str, columns: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let word_len = word.chars().count();
        let current_len = current.chars().count();
        if current.is_empty() {
            if word_len <= columns {
                current.push_str(word);
            } else {
                let mut chunk = String::new();
                for ch in word.chars() {
                    if chunk.chars().count() == columns {
                        lines.push(chunk);
                        chunk = String::new();
                    }
                    chunk.push(ch);
                }
                current = chunk;
            }
        } else if current_len + 1 + word_len <= columns {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(current);
            current = word.to_owned();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn center(out: &mut String, text: &str, columns: usize) {
    for part in wrap(text, columns) {
        let len = part.chars().count();
        let pad = columns.saturating_sub(len) / 2;
        line(out, &format!("{}{part}", " ".repeat(pad)));
    }
}

fn two_columns(out: &mut String, left: &str, right: &str, columns: usize) {
    let left_len = left.chars().count();
    let right_len = right.chars().count();
    if left_len + right_len < columns {
        let pad = columns - left_len - right_len;
        line(out, &format!("{left}{}{right}", " ".repeat(pad)));
    } else {
        line(out, left);
        let pad = columns.saturating_sub(right_len);
        line(out, &format!("{}{right}", " ".repeat(pad)));
    }
}

fn push_item(out: &mut String, item: &ReceiptItem, columns: usize) {
    for part in wrap(&item.name, columns) {
        line(out, &part);
    }
    let left = format!("  {} x {}", item.quantity, item.unit_price);
    two_columns(out, &left, &item.total.to_string(), columns);
    for detail in &item.details {
        for (index, part) in wrap(detail, columns.saturating_sub(4)).iter().enumerate() {
            let prefix = if index == 0 { "    - " } else { "      " };
            line(out, &format!("{prefix}{part}"));
        }
    }
}

/// Renders the customer receipt as text at the printer's column width.
#[must_use]
pub fn render_receipt(receipt: &Receipt, width: PaperWidth) -> String {
    let columns = width.columns();
    let mut out = String::new();

    center(&mut out, &receipt.business_name, columns);
    if !receipt.business_address.trim().is_empty() {
        center(&mut out, &receipt.business_address, columns);
    }
    if !receipt.business_phone.trim().is_empty() {
        center(&mut out, &receipt.business_phone, columns);
    }
    rule(&mut out, columns, '=');

    if receipt.duplicate {
        center(&mut out, "*** DUPLICATE ***", columns);
        rule(&mut out, columns, '=');
    }

    two_columns(
        &mut out,
        &format!("Order: {}", receipt.order_no),
        "",
        columns,
    );
    let order_type = match &receipt.table_no {
        Some(table) => format!("Type: {}  Table {table}", receipt.order_type),
        None => format!("Type: {}", receipt.order_type),
    };
    line(&mut out, &order_type);
    line(&mut out, &format!("Cashier: {}", receipt.cashier));
    line(&mut out, &format!("Time: {}", receipt.created_at));
    rule(&mut out, columns, '-');

    for item in &receipt.items {
        push_item(&mut out, item, columns);
    }
    rule(&mut out, columns, '-');

    two_columns(&mut out, "Subtotal", &receipt.subtotal.to_string(), columns);
    if receipt.packaging_fee.kobo() > 0 {
        two_columns(
            &mut out,
            "Packaging",
            &receipt.packaging_fee.to_string(),
            columns,
        );
    }
    two_columns(&mut out, "TOTAL", &receipt.total.to_string(), columns);
    rule(&mut out, columns, '-');

    for payment in &receipt.payments {
        two_columns(
            &mut out,
            &payment.method,
            &payment.amount.to_string(),
            columns,
        );
    }
    if let Some(change) = receipt.change {
        two_columns(&mut out, "Change", &change.to_string(), columns);
    }
    rule(&mut out, columns, '=');

    if !receipt.footer.trim().is_empty() {
        center(&mut out, &receipt.footer, columns);
        rule(&mut out, columns, '=');
    }

    out
}

/// Renders the kitchen/hand-out slip, which carries no prices.
#[must_use]
pub fn render_slip(slip: &Slip, width: PaperWidth) -> String {
    let columns = width.columns();
    let mut out = String::new();

    center(&mut out, &slip.business_name, columns);
    center(&mut out, "SERVING SLIP", columns);
    rule(&mut out, columns, '=');
    line(&mut out, &format!("Order: {}", slip.order_no));
    match &slip.table_no {
        Some(table) => line(
            &mut out,
            &format!("Type: {}  Table {table}", slip.order_type),
        ),
        None => line(&mut out, &format!("Type: {}", slip.order_type)),
    }
    line(&mut out, &format!("Time: {}", slip.created_at));
    rule(&mut out, columns, '-');

    for (name, quantity, details) in &slip.lines {
        for part in wrap(name, columns) {
            line(&mut out, &format!("{quantity} x {part}"));
        }
        for detail in details {
            for (index, part) in wrap(detail, columns.saturating_sub(4)).iter().enumerate() {
                let prefix = if index == 0 { "    - " } else { "      " };
                line(&mut out, &format!("{prefix}{part}"));
            }
        }
    }
    rule(&mut out, columns, '=');

    out
}

/// Wraps receipt text in the ESC/POS init and cut commands used by common printers.
#[must_use]
pub fn escpos(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len() + 12);
    bytes.extend_from_slice(b"\x1b@");
    bytes.extend_from_slice(text.as_bytes());
    bytes.extend_from_slice(b"\n\n\n\x1dV\x00");
    bytes
}

#[cfg(test)]
mod tests {
    use super::{
        escpos, render_receipt, render_slip, PaperWidth, Receipt, ReceiptItem, ReceiptPayment, Slip,
    };
    use pos_core::Money;

    fn sample() -> Receipt {
        Receipt {
            business_name: "Hiffey Restaurant".to_owned(),
            business_address: "12 Market Road".to_owned(),
            business_phone: "08030000000".to_owned(),
            footer: "Thank you".to_owned(),
            order_no: "HIF-20261111-0001".to_owned(),
            order_type: "Dine-in".to_owned(),
            table_no: Some("4".to_owned()),
            cashier: "Chidi".to_owned(),
            created_at: "11/11/2026 14:05".to_owned(),
            items: vec![
                ReceiptItem {
                    name: "Jollof Rice".to_owned(),
                    quantity: 2,
                    unit_price: Money::from_kobo(35_000),
                    total: Money::from_kobo(70_000),
                    details: Vec::new(),
                },
                ReceiptItem {
                    name: "Standard Pack".to_owned(),
                    quantity: 1,
                    unit_price: Money::from_kobo(150_000),
                    total: Money::from_kobo(150_000),
                    details: vec!["2 spoons Jollof Rice".to_owned(), "1 piece Coke".to_owned()],
                },
            ],
            subtotal: Money::from_kobo(220_000),
            packaging_fee: Money::from_kobo(20_000),
            total: Money::from_kobo(240_000),
            payments: vec![ReceiptPayment {
                method: "Cash".to_owned(),
                amount: Money::from_kobo(250_000),
            }],
            change: Some(Money::from_kobo(10_000)),
            duplicate: false,
        }
    }

    #[test]
    fn every_line_fits_the_paper() {
        for width in [PaperWidth::Mm58, PaperWidth::Mm80] {
            let text = render_receipt(&sample(), width);
            for line in text.lines() {
                assert!(
                    line.chars().count() <= width.columns(),
                    "line too wide: {line}"
                );
            }
        }
    }

    #[test]
    fn receipt_carries_the_required_fields() {
        let text = render_receipt(&sample(), PaperWidth::Mm58);
        for expected in [
            "Hiffey Restaurant",
            "HIF-20261111-0001",
            "Dine-in",
            "Table 4",
            "Jollof Rice",
            "Standard Pack",
            "2 spoons Jollof Rice",
            "Cashier: Chidi",
            "TOTAL",
            "\u{20a6}2400.00",
            "Cash",
            "Change",
            "Thank you",
        ] {
            assert!(text.contains(expected), "missing {expected} in\n{text}");
        }
        assert!(!text.contains("DUPLICATE"));
    }

    #[test]
    fn duplicates_are_labelled_and_packaging_hidden_when_zero() {
        let mut receipt = sample();
        receipt.duplicate = true;
        receipt.packaging_fee = Money::ZERO;
        let text = render_receipt(&receipt, PaperWidth::Mm80);
        assert!(text.contains("*** DUPLICATE ***"));
        assert!(!text.contains("Packaging"));
    }

    #[test]
    fn slip_lists_quantities_without_prices() {
        let slip = Slip {
            business_name: "Hiffey Restaurant".to_owned(),
            order_no: "HIF-20261111-0002".to_owned(),
            order_type: "Takeaway".to_owned(),
            table_no: None,
            created_at: "11/11/2026 14:10".to_owned(),
            lines: vec![(
                "Standard Pack".to_owned(),
                1,
                vec!["2 spoons Jollof Rice".to_owned()],
            )],
        };
        let text = render_slip(&slip, PaperWidth::Mm58);
        assert!(text.contains("SERVING SLIP"));
        assert!(text.contains("1 x Standard Pack"));
        assert!(text.contains("2 spoons Jollof Rice"));
        assert!(!text.contains('\u{20a6}'));
        for line in text.lines() {
            assert!(line.chars().count() <= 32);
        }
    }

    #[test]
    fn escpos_wraps_with_init_and_cut() {
        let bytes = escpos("hello");
        assert!(bytes.starts_with(b"\x1b@"));
        assert!(bytes.ends_with(b"\x1dV\x00"));
    }
}
