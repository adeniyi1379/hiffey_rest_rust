//! Money is always stored as integer kobo. No floating point is allowed in pricing.

use std::fmt;
use std::ops::{Add, AddAssign, Sub, SubAssign};

/// An amount of money, stored as a whole number of kobo (1 naira = 100 kobo).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Money(i64);

impl Money {
    pub const ZERO: Self = Self(0);

    #[must_use]
    pub const fn from_kobo(kobo: i64) -> Self {
        Self(kobo)
    }

    #[must_use]
    pub const fn kobo(self) -> i64 {
        self.0
    }

    /// Divides `numerator` by `denominator`, rounding half up to the nearest kobo.
    ///
    /// Used for tax and proportional splits, where a single rounding per step is required.
    ///
    /// # Panics
    ///
    /// Panics if `denominator` is zero.
    #[must_use]
    pub fn round_half_up_div(numerator: i128, denominator: i128) -> Self {
        assert!(denominator != 0, "denominator must not be zero");
        let negative = (numerator < 0) != (denominator < 0);
        let numerator = numerator.abs();
        let denominator = denominator.abs();
        let quotient = numerator / denominator;
        let remainder = numerator % denominator;
        let rounded = if remainder * 2 >= denominator {
            quotient + 1
        } else {
            quotient
        };
        let rounded = i64::try_from(rounded).expect("money overflow");
        Self(if negative { -rounded } else { rounded })
    }

    #[must_use]
    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        self.0.checked_add(rhs.0).map(Self)
    }

    #[must_use]
    pub fn checked_sub(self, rhs: Self) -> Option<Self> {
        self.0.checked_sub(rhs.0).map(Self)
    }

    /// Multiplies this amount by a whole quantity (spoons, pieces, bottles).
    #[must_use]
    pub fn checked_mul_quantity(self, quantity: i64) -> Option<Self> {
        self.0.checked_mul(quantity).map(Self)
    }
}

impl Add for Money {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl AddAssign for Money {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl Sub for Money {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl SubAssign for Money {
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let kobo = self.0.abs();
        write!(f, "{sign}\u{20a6}{}.{:02}", kobo / 100, kobo % 100)
    }
}

#[cfg(test)]
mod tests {
    use super::Money;

    #[test]
    fn kobo_round_trips() {
        assert_eq!(Money::from_kobo(12_345).kobo(), 12_345);
    }

    #[test]
    fn addition_and_subtraction_are_exact() {
        let a = Money::from_kobo(1_000);
        let b = Money::from_kobo(250);
        assert_eq!((a + b).kobo(), 1_250);
        assert_eq!((a - b).kobo(), 750);
    }

    #[test]
    fn multiplication_uses_whole_quantities() {
        let spoon = Money::from_kobo(35_000);
        assert_eq!(spoon.checked_mul_quantity(3).unwrap().kobo(), 105_000);
    }

    #[test]
    fn rounds_half_up() {
        assert_eq!(Money::round_half_up_div(5, 2).kobo(), 3);
        assert_eq!(Money::round_half_up_div(3, 2).kobo(), 2);
        assert_eq!(Money::round_half_up_div(1, 2).kobo(), 1);
        assert_eq!(Money::round_half_up_div(1, 3).kobo(), 0);
    }

    #[test]
    fn rounding_keeps_sign() {
        assert_eq!(Money::round_half_up_div(-5, 2).kobo(), -3);
        assert_eq!(Money::round_half_up_div(-1, 2).kobo(), -1);
    }

    #[test]
    fn displays_in_naira() {
        assert_eq!(Money::from_kobo(105_000).to_string(), "\u{20a6}1050.00");
        assert_eq!(Money::from_kobo(-150).to_string(), "-\u{20a6}1.50");
    }
}
