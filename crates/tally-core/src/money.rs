//! Money as integer minor units. Never floats.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::iter::Sum;
use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};

/// An amount in minor units (cents for USD). Signed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Cents(pub i64);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseMoneyError {
    #[error("empty amount")]
    Empty,
    #[error("invalid amount: {0:?}")]
    Invalid(String),
    #[error("more than two decimal places: {0:?}")]
    TooPrecise(String),
    #[error("amount out of range: {0:?}")]
    Overflow(String),
}

impl Cents {
    pub const ZERO: Cents = Cents(0);

    pub fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub fn abs(self) -> Cents {
        Cents(self.0.abs())
    }

    pub fn checked_add(self, other: Cents) -> Option<Cents> {
        self.0.checked_add(other.0).map(Cents)
    }

    /// Parses human/bank formats: `1234.5`, `-1,234.56`, `$12`, `(12.00)`, `+3.10`.
    pub fn parse(input: &str) -> Result<Cents, ParseMoneyError> {
        let s = input.trim();
        if s.is_empty() {
            return Err(ParseMoneyError::Empty);
        }
        let invalid = || ParseMoneyError::Invalid(input.to_string());

        let (mut negative, mut s) = match s.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
            Some(inner) => (true, inner.trim()),
            None => (false, s),
        };
        if let Some(rest) = s.strip_prefix('-') {
            if negative {
                return Err(invalid());
            }
            negative = true;
            s = rest;
        } else if let Some(rest) = s.strip_prefix('+') {
            s = rest;
        }
        let s = s.strip_prefix('$').unwrap_or(s).replace(',', "");

        let (whole, frac) = match s.split_once('.') {
            Some((w, f)) => (w, f),
            None => (s.as_str(), ""),
        };
        if whole.is_empty() && frac.is_empty() {
            return Err(invalid());
        }
        if !whole.chars().all(|c| c.is_ascii_digit()) || !frac.chars().all(|c| c.is_ascii_digit()) {
            return Err(invalid());
        }
        if frac.len() > 2 {
            return Err(ParseMoneyError::TooPrecise(input.to_string()));
        }

        let overflow = || ParseMoneyError::Overflow(input.to_string());
        let whole: i64 = if whole.is_empty() { 0 } else { whole.parse().map_err(|_| overflow())? };
        let frac: i64 = match frac.len() {
            0 => 0,
            1 => frac.parse::<i64>().unwrap() * 10,
            _ => frac.parse().unwrap(),
        };
        let cents = whole.checked_mul(100).and_then(|w| w.checked_add(frac)).ok_or_else(overflow)?;
        Ok(Cents(if negative { -cents } else { cents }))
    }
}

/// `-1,234.56` — no currency symbol; the UI owns presentation.
impl fmt::Display for Cents {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let abs = self.0.unsigned_abs();
        let whole = (abs / 100).to_string();
        let mut grouped = String::with_capacity(whole.len() + whole.len() / 3);
        for (i, ch) in whole.chars().enumerate() {
            if i > 0 && (whole.len() - i).is_multiple_of(3) {
                grouped.push(',');
            }
            grouped.push(ch);
        }
        write!(f, "{sign}{grouped}.{:02}", abs % 100)
    }
}

impl Add for Cents {
    type Output = Cents;
    fn add(self, rhs: Cents) -> Cents {
        Cents(self.0 + rhs.0)
    }
}

impl Sub for Cents {
    type Output = Cents;
    fn sub(self, rhs: Cents) -> Cents {
        Cents(self.0 - rhs.0)
    }
}

impl Neg for Cents {
    type Output = Cents;
    fn neg(self) -> Cents {
        Cents(-self.0)
    }
}

impl AddAssign for Cents {
    fn add_assign(&mut self, rhs: Cents) {
        self.0 += rhs.0;
    }
}

impl SubAssign for Cents {
    fn sub_assign(&mut self, rhs: Cents) {
        self.0 -= rhs.0;
    }
}

impl Sum for Cents {
    fn sum<I: Iterator<Item = Cents>>(iter: I) -> Cents {
        iter.fold(Cents::ZERO, |a, b| a + b)
    }
}

impl<'a> Sum<&'a Cents> for Cents {
    fn sum<I: Iterator<Item = &'a Cents>>(iter: I) -> Cents {
        iter.copied().sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_formats() {
        assert_eq!(Cents::parse("12").unwrap(), Cents(1200));
        assert_eq!(Cents::parse("12.5").unwrap(), Cents(1250));
        assert_eq!(Cents::parse("-1,234.56").unwrap(), Cents(-123456));
        assert_eq!(Cents::parse("$0.07").unwrap(), Cents(7));
        assert_eq!(Cents::parse("(12.00)").unwrap(), Cents(-1200));
        assert_eq!(Cents::parse("+3.10").unwrap(), Cents(310));
        assert_eq!(Cents::parse(".99").unwrap(), Cents(99));
        assert_eq!(Cents::parse(" -$5 ").unwrap(), Cents(-500));
    }

    #[test]
    fn rejects_garbage() {
        assert!(matches!(Cents::parse(""), Err(ParseMoneyError::Empty)));
        assert!(Cents::parse("abc").is_err());
        assert!(Cents::parse("1.2.3").is_err());
        assert!(Cents::parse(".").is_err());
        assert!(Cents::parse("(-5)").is_err());
        assert!(matches!(Cents::parse("1.234"), Err(ParseMoneyError::TooPrecise(_))));
        assert!(matches!(Cents::parse("99999999999999999999"), Err(ParseMoneyError::Overflow(_))));
    }

    #[test]
    fn displays_grouped() {
        assert_eq!(Cents(0).to_string(), "0.00");
        assert_eq!(Cents(5).to_string(), "0.05");
        assert_eq!(Cents(-123456).to_string(), "-1,234.56");
        assert_eq!(Cents(100_000_000).to_string(), "1,000,000.00");
        assert_eq!(Cents(i64::MIN).to_string(), "-92,233,720,368,547,758.08");
    }
}
