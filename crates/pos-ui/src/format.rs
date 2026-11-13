//! Display formatting that is shared by every screen and testable on the host.

use pos_types::UnitType;

/// Formats kobo as naira with thousands separators and two decimals, e.g. `"₦12,500.00"`.
#[must_use]
pub fn money(kobo: i64) -> String {
    let sign = if kobo < 0 { "-" } else { "" };
    let value = kobo.abs();
    let naira = value / 100;
    let kopeks = value % 100;
    let digits = naira.to_string();
    let mut grouped = String::new();
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    format!("{sign}\u{20a6}{grouped}.{kopeks:02}")
}

/// Parses user-entered naira (with optional ₦, commas, spaces and 0-2 decimals) into kobo.
#[must_use]
pub fn parse_naira(input: &str) -> Option<i64> {
    let cleaned: String = input
        .chars()
        .filter(|ch| !matches!(ch, '\u{20a6}' | ',' | ' ' | '_'))
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    let (whole, fraction) = match cleaned.split_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => (cleaned.as_str(), ""),
    };
    if whole.is_empty() && fraction.is_empty() {
        return None;
    }
    if !whole.chars().all(|ch| ch.is_ascii_digit()) || fraction.len() > 2 {
        return None;
    }
    if !fraction.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let naira: i64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let kopeks: i64 = match fraction.len() {
        0 => 0,
        1 => fraction.parse::<i64>().ok()? * 10,
        _ => fraction.parse().ok()?,
    };
    naira.checked_mul(100)?.checked_add(kopeks)
}

/// `"2 spoons"`, `"1 piece"` and so on.
#[must_use]
pub fn quantity_label(quantity: i64, unit: UnitType) -> String {
    let suffix = if quantity == 1 { "" } else { "s" };
    format!("{quantity} {}{suffix}", unit.label())
}

/// Short local time (WAT) for chips and lists; falls back to the raw value.
#[must_use]
pub fn short_time(rfc3339: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .map(|at| {
            (at.with_timezone(&chrono::Utc) + chrono::Duration::hours(1))
                .format("%d/%m %H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| rfc3339.to_owned())
}

/// Time of day only, used in the top bar clock (UTC + 1, the shop's day).
#[must_use]
pub fn clock(now: &chrono::DateTime<chrono::Utc>) -> String {
    (*now + chrono::Duration::hours(1))
        .format("%H:%M")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{money, parse_naira, quantity_label};
    use pos_types::UnitType;

    #[test]
    fn money_groups_thousands() {
        assert_eq!(money(0), "\u{20a6}0.00");
        assert_eq!(money(5), "\u{20a6}0.05");
        assert_eq!(money(70_000), "\u{20a6}700.00");
        assert_eq!(money(125_005), "\u{20a6}1,250.05");
        assert_eq!(money(123_456_789), "\u{20a6}1,234,567.89");
        assert_eq!(money(-2_500), "-\u{20a6}25.00");
    }

    #[test]
    fn naira_input_round_trips() {
        assert_eq!(parse_naira("700"), Some(70_000));
        assert_eq!(parse_naira("\u{20a6}1,250.05"), Some(125_005));
        assert_eq!(parse_naira("0.5"), Some(50));
        assert_eq!(parse_naira("0.05"), Some(5));
        assert_eq!(parse_naira(" . "), None);
        assert_eq!(parse_naira("12.345"), None);
        assert_eq!(parse_naira("abc"), None);
    }

    #[test]
    fn quantities_use_the_shop_words() {
        assert_eq!(quantity_label(1, UnitType::Spoon), "1 spoon");
        assert_eq!(quantity_label(3, UnitType::Spoon), "3 spoons");
        assert_eq!(quantity_label(1, UnitType::Count), "1 piece");
        assert_eq!(quantity_label(2, UnitType::Count), "2 pieces");
    }
}
