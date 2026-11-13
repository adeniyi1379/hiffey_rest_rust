//! Pricing rules: line totals, pack totals and the order total.
//!
//! The sequence is fixed: line price, then packaging fee. (Discounts and VAT are
//! intentionally not part of this build.)

use crate::Money;

/// A line total is quantity times the unit price, in whole units.
///
/// Returns `None` on overflow or when the quantity is not positive.
#[must_use]
pub fn line_total(unit_price: Money, quantity: i64) -> Option<Money> {
    if quantity <= 0 {
        return None;
    }
    unit_price.checked_mul_quantity(quantity)
}

/// The calculated price of a pack: the sum of each component's line total.
#[must_use]
pub fn calculated_pack_price(components: &[(Money, i64)]) -> Option<Money> {
    let mut total = Money::ZERO;
    for (unit_price, quantity) in components {
        total = total.checked_add(line_total(*unit_price, *quantity)?)?;
    }
    Some(total)
}

/// The amount charged for a pack: the fixed price if the manager set one,
/// otherwise the calculated sum.
#[must_use]
pub fn pack_price(
    pricing_mode_is_fixed: bool,
    fixed_price: Option<Money>,
    components: &[(Money, i64)],
) -> Option<Money> {
    match (pricing_mode_is_fixed, fixed_price) {
        (true, Some(price)) => Some(price),
        (true, None) => None,
        (false, _) => calculated_pack_price(components),
    }
}

/// Add the takeaway packaging fee to the sum of the lines.
#[must_use]
pub fn order_total(lines: &[Money], packaging_fee: Money) -> Option<Money> {
    let mut total = packaging_fee;
    for line in lines {
        total = total.checked_add(*line)?;
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::{calculated_pack_price, line_total, order_total, pack_price};
    use crate::Money;

    fn naira(amount: i64) -> Money {
        Money::from_kobo(amount * 100)
    }

    #[test]
    fn line_total_multiplies_whole_units() {
        assert_eq!(line_total(naira(350), 3).unwrap().kobo(), 105_000);
        assert_eq!(line_total(naira(50), 1).unwrap().kobo(), 5_000);
    }

    #[test]
    fn line_total_rejects_zero_and_negative_quantities() {
        assert!(line_total(naira(100), 0).is_none());
        assert!(line_total(naira(100), -2).is_none());
    }

    #[test]
    fn standard_pack_adds_up() {
        let pack = calculated_pack_price(&[
            (naira(350), 2),
            (naira(400), 2),
            (naira(50), 5),
            (naira(200), 1),
            (naira(150), 1),
        ])
        .unwrap();
        assert_eq!(pack.kobo(), 210_000);
    }

    #[test]
    fn fixed_pack_price_overrides_the_sum() {
        let components = [(naira(350), 2), (naira(150), 1)];
        assert_eq!(
            pack_price(true, Some(naira(800)), &components)
                .unwrap()
                .kobo(),
            80_000
        );
        assert!(pack_price(true, None, &components).is_none());
        assert_eq!(
            pack_price(false, Some(naira(800)), &components)
                .unwrap()
                .kobo(),
            85_000
        );
    }

    #[test]
    fn packaging_fee_is_added_once_per_order() {
        let lines = [naira(1_050), naira(500)];
        assert_eq!(order_total(&lines, naira(200)).unwrap().kobo(), 175_000);
        assert_eq!(order_total(&[], Money::ZERO).unwrap(), Money::ZERO);
    }
}
