use rust_decimal::{Decimal, RoundingStrategy, prelude::ToPrimitive};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Money(i64);

impl Money {
    pub const ZERO: Self = Self(0);

    /// Creates a non-negative money value from its persisted integer cents.
    ///
    /// # Errors
    ///
    /// Returns [`CalculationError::NegativeMoney`] when `cents` is negative.
    pub fn from_cents(cents: i64) -> Result<Self, CalculationError> {
        if cents < 0 {
            return Err(CalculationError::NegativeMoney);
        }
        Ok(Self(cents))
    }

    #[must_use]
    pub const fn cents(self) -> i64 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LineCalculationInput {
    pub quantity: Decimal,
    pub unit_price: Money,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentTotals {
    pub line_totals: Vec<Money>,
    pub subtotal: Money,
    pub discount: Money,
    pub tax: Money,
    pub total: Money,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum CalculationError {
    #[error("money cannot be negative")]
    NegativeMoney,
    #[error("quantity must be zero or greater")]
    NegativeQuantity,
    #[error("tax basis points must be between 0 and 10000")]
    InvalidTaxRate,
    #[error("calculated money amount is outside the supported integer-cent range")]
    Overflow,
}

/// Calculates rounded line totals and the document subtotal, discount, tax, and total.
///
/// # Errors
///
/// Returns an error for negative quantities, invalid tax basis points, or arithmetic overflow.
pub fn calculate_document(
    lines: &[LineCalculationInput],
    requested_discount: Money,
    tax_basis_points: u32,
) -> Result<DocumentTotals, CalculationError> {
    if tax_basis_points > 10_000 {
        return Err(CalculationError::InvalidTaxRate);
    }

    let mut subtotal_cents = 0_i64;
    let mut line_totals = Vec::with_capacity(lines.len());
    for line in lines {
        if line.quantity.is_sign_negative() {
            return Err(CalculationError::NegativeQuantity);
        }
        let amount = Decimal::from(line.unit_price.cents()) * line.quantity;
        let cents = rounded_cents(amount)?;
        subtotal_cents = subtotal_cents
            .checked_add(cents)
            .ok_or(CalculationError::Overflow)?;
        line_totals.push(Money(cents));
    }

    let discount_cents = requested_discount.cents().min(subtotal_cents);
    let taxable_cents = subtotal_cents - discount_cents;
    let tax_decimal =
        Decimal::from(taxable_cents) * Decimal::from(tax_basis_points) / Decimal::from(10_000_u32);
    let tax_cents = rounded_cents(tax_decimal)?;
    let total_cents = taxable_cents
        .checked_add(tax_cents)
        .ok_or(CalculationError::Overflow)?;

    Ok(DocumentTotals {
        line_totals,
        subtotal: Money(subtotal_cents),
        discount: Money(discount_cents),
        tax: Money(tax_cents),
        total: Money(total_cents),
    })
}

fn rounded_cents(value: Decimal) -> Result<i64, CalculationError> {
    value
        .round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero)
        .to_i64()
        .ok_or(CalculationError::Overflow)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn money(cents: i64) -> Money {
        Money::from_cents(cents).unwrap_or_else(|error| panic!("{error}"))
    }

    #[test]
    fn fractional_quantities_discount_and_tax_are_exact() {
        let lines = [
            LineCalculationInput {
                quantity: Decimal::from_str("2.5").unwrap_or_else(|error| panic!("{error}")),
                unit_price: money(1_999),
            },
            LineCalculationInput {
                quantity: Decimal::from_str("0.125").unwrap_or_else(|error| panic!("{error}")),
                unit_price: money(8_000),
            },
        ];

        let totals =
            calculate_document(&lines, money(500), 825).unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(totals.line_totals, vec![money(4_998), money(1_000)]);
        assert_eq!(totals.subtotal, money(5_998));
        assert_eq!(totals.discount, money(500));
        assert_eq!(totals.tax, money(454));
        assert_eq!(totals.total, money(5_952));
    }

    #[test]
    fn rounds_half_cents_away_from_zero_and_caps_discount() {
        let lines = [LineCalculationInput {
            quantity: Decimal::from_str("0.5").unwrap_or_else(|error| panic!("{error}")),
            unit_price: money(1),
        }];
        let totals =
            calculate_document(&lines, money(99), 0).unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(totals.line_totals, vec![money(1)]);
        assert_eq!(totals.discount, money(1));
        assert_eq!(totals.total, Money::ZERO);
    }
}
