//! Order numbers: `HIF-{yyyyMMdd}-{sequence}`, shared by both terminals.
//!
//! The sequence is per trading day in West Africa Time (UTC+1) and is allocated
//! by the server inside the sale transaction.

/// Fixed prefix on every order number.
pub const ORDER_PREFIX: &str = "HIF";

/// Formats the order number for a trading day and sequence number.
///
/// `day` must already be the WAT calendar day as `YYYYMMDD`; `sequence` starts at 1.
///
/// # Panics
///
/// Panics if `day` is not eight ASCII digits or `sequence` is not positive.
#[must_use]
pub fn format_order_no(day: &str, sequence: i64) -> String {
    assert!(
        day.len() == 8 && day.chars().all(|ch| ch.is_ascii_digit()),
        "day must be YYYYMMDD"
    );
    assert!(sequence > 0, "sequence must be positive");
    format!("{ORDER_PREFIX}-{day}-{sequence:04}")
}

#[cfg(test)]
mod tests {
    use super::format_order_no;

    #[test]
    fn first_order_of_the_day() {
        assert_eq!(format_order_no("20261110", 1), "HIF-20261110-0001");
    }

    #[test]
    fn sequence_grows_past_four_digits() {
        assert_eq!(format_order_no("20261110", 12_345), "HIF-20261110-12345");
    }

    #[test]
    #[should_panic(expected = "day must be YYYYMMDD")]
    fn rejects_a_malformed_day() {
        let _ = format_order_no("2026-11-10", 1);
    }

    #[test]
    #[should_panic(expected = "sequence must be positive")]
    fn rejects_zero_sequence() {
        let _ = format_order_no("20261110", 0);
    }
}
