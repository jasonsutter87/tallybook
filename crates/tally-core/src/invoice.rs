//! Invoice & bill arithmetic and receivables/payables aging.
//!
//! Quantities are integer thousandths (1.5 hours = 1500) and tax rates are
//! basis points (8.25% = 825), so every total is exact integer math with a
//! single, explicit rounding step (half away from zero).

use crate::money::Cents;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// `n / d` rounded half away from zero. `d` must be positive.
pub fn div_round(n: i128, d: i128) -> i128 {
    debug_assert!(d > 0);
    let q = n / d;
    let r = n % d;
    if 2 * r.abs() >= d { q + n.signum() } else { q }
}

/// quantity (thousandths) × unit price → line amount.
pub fn line_amount(quantity_milli: i64, unit_price: Cents) -> Option<Cents> {
    let v = div_round(i128::from(quantity_milli) * i128::from(unit_price.0), 1000);
    i64::try_from(v).ok().map(Cents)
}

pub fn tax_on(taxable: Cents, rate_bp: u32) -> Option<Cents> {
    let v = div_round(i128::from(taxable.0) * i128::from(rate_bp), 10_000);
    i64::try_from(v).ok().map(Cents)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineInput {
    pub quantity_milli: i64,
    pub unit_price: Cents,
    pub taxable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Totals {
    pub subtotal: Cents,
    pub taxable: Cents,
    pub tax: Cents,
    pub total: Cents,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvoiceMathError {
    #[error("line {0}: amount is too large")]
    Overflow(usize),
    #[error("line {0}: quantity must be positive")]
    BadQuantity(usize),
}

pub fn totals(lines: &[LineInput], tax_rate_bp: u32) -> Result<Totals, InvoiceMathError> {
    let mut t = Totals::default();
    for (i, l) in lines.iter().enumerate() {
        if l.quantity_milli <= 0 {
            return Err(InvoiceMathError::BadQuantity(i));
        }
        let amount = line_amount(l.quantity_milli, l.unit_price).ok_or(InvoiceMathError::Overflow(i))?;
        t.subtotal = t.subtotal.checked_add(amount).ok_or(InvoiceMathError::Overflow(i))?;
        if l.taxable {
            t.taxable = t.taxable.checked_add(amount).ok_or(InvoiceMathError::Overflow(i))?;
        }
    }
    t.tax = tax_on(t.taxable, tax_rate_bp).ok_or(InvoiceMathError::Overflow(lines.len()))?;
    t.total = t.subtotal.checked_add(t.tax).ok_or(InvoiceMathError::Overflow(lines.len()))?;
    Ok(t)
}

/// Days-past-due buckets, as on every A/R aging report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct AgingBuckets {
    pub current: Cents,
    pub days_1_30: Cents,
    pub days_31_60: Cents,
    pub days_61_90: Cents,
    pub over_90: Cents,
}

impl AgingBuckets {
    pub fn add(&mut self, due: NaiveDate, as_of: NaiveDate, open: Cents) {
        let late = (as_of - due).num_days();
        let bucket = match late {
            i64::MIN..=0 => &mut self.current,
            1..=30 => &mut self.days_1_30,
            31..=60 => &mut self.days_31_60,
            61..=90 => &mut self.days_61_90,
            _ => &mut self.over_90,
        };
        *bucket += open;
    }

    pub fn total(&self) -> Cents {
        self.current + self.days_1_30 + self.days_31_60 + self.days_61_90 + self.over_90
    }
}

/// Parses "1.5" → 1500 thousandths.
pub fn parse_quantity(s: &str) -> Option<i64> {
    let s = s.trim();
    let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
    if whole.is_empty() && frac.is_empty() || frac.len() > 3 {
        return None;
    }
    if !whole.chars().all(|c| c.is_ascii_digit()) || !frac.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let w: i64 = if whole.is_empty() { 0 } else { whole.parse().ok()? };
    let f: i64 = format!("{frac:0<3}").parse().ok()?;
    w.checked_mul(1000)?.checked_add(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, m, day).unwrap()
    }

    #[test]
    fn rounding_is_half_away_from_zero() {
        assert_eq!(div_round(5, 10), 1);
        assert_eq!(div_round(4, 10), 0);
        assert_eq!(div_round(-5, 10), -1);
        assert_eq!(div_round(15, 10), 2);
    }

    #[test]
    fn totals_with_tax() {
        let lines = [
            LineInput { quantity_milli: 1500, unit_price: Cents(12_000), taxable: false }, // 1.5h × $120
            LineInput { quantity_milli: 3000, unit_price: Cents(333), taxable: true },     // 3 × $3.33
        ];
        let t = totals(&lines, 825).unwrap();
        assert_eq!(t.subtotal, Cents(18_000 + 999));
        assert_eq!(t.taxable, Cents(999));
        assert_eq!(t.tax, Cents(82)); // 9.99 × 8.25% = 0.824 → 0.82
        assert_eq!(t.total, Cents(19_081));
    }

    #[test]
    fn fractional_quantities_round_once() {
        assert_eq!(line_amount(333, Cents(100)), Some(Cents(33)));
        assert_eq!(line_amount(335, Cents(100)), Some(Cents(34)));
        assert!(totals(&[LineInput { quantity_milli: 0, unit_price: Cents(1), taxable: false }], 0).is_err());
    }

    #[test]
    fn aging_buckets() {
        let mut a = AgingBuckets::default();
        a.add(d(3, 31), d(3, 31), Cents(100));
        a.add(d(3, 1), d(3, 31), Cents(200));
        a.add(d(1, 1), d(3, 31), Cents(300));
        a.add(d(10, 1), d(3, 31), Cents(400));
        assert_eq!(a.current, Cents(500));
        assert_eq!(a.days_1_30, Cents(200));
        assert_eq!(a.days_61_90, Cents(300));
        assert_eq!(a.total(), Cents(1000));
    }

    #[test]
    fn quantities_parse() {
        assert_eq!(parse_quantity("1.5"), Some(1500));
        assert_eq!(parse_quantity("2"), Some(2000));
        assert_eq!(parse_quantity(".25"), Some(250));
        assert_eq!(parse_quantity("1.2345"), None);
        assert_eq!(parse_quantity("-1"), None);
    }
}
