//! When recurring bills fall due.
//!
//! A bill repeats monthly, every few months, yearly, or weekly. Monthly-ish
//! bills land on a day of the month, moved back to the month's last day when
//! the month is short (a bill due on the 31st is due April 30th).

use chrono::{Datelike, Days, Months, NaiveDate};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "every", rename_all = "snake_case")]
pub enum Cadence {
    /// On `day` every month.
    Month { day: u32 },
    /// On `day` every `months` months, counting from the bill's start month.
    Months { months: u32, day: u32 },
    /// Every 7 days from the bill's start date.
    Week,
}

impl Cadence {
    /// Rough count per year, for turning a bill into a monthly amount.
    pub fn per_year(self) -> u32 {
        match self {
            Cadence::Month { .. } => 12,
            Cadence::Months { months, .. } => 12 / months.clamp(1, 12),
            Cadence::Week => 52,
        }
    }

    pub fn validate(self) -> Result<(), &'static str> {
        match self {
            Cadence::Month { day } | Cadence::Months { day, .. } if !(1..=31).contains(&day) => Err("a due day is 1 to 31"),
            Cadence::Months { months, .. } if !(1..=24).contains(&months) => Err("repeat every 1 to 24 months"),
            _ => Ok(()),
        }
    }
}

fn last_day(year: i32, month: u32) -> u32 {
    let first = NaiveDate::from_ymd_opt(year, month, 1).expect("valid month");
    (first + Months::new(1) - Days::new(1)).day()
}

/// `day` of the given month, moved back to the last day when it doesn't exist.
pub fn day_in_month(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day.clamp(1, last_day(year, month))).expect("clamped day is valid")
}

fn month_index(d: NaiveDate) -> i64 {
    i64::from(d.year()) * 12 + i64::from(d.month0())
}

/// Every due date in `[from, to]`, oldest first, never before `start`.
pub fn occurrences(cadence: Cadence, start: NaiveDate, from: NaiveDate, to: NaiveDate) -> Vec<NaiveDate> {
    let from = from.max(start);
    let mut out = Vec::new();
    if from > to {
        return out;
    }
    match cadence {
        Cadence::Week => {
            let gap = (from - start).num_days();
            let mut d = start + Days::new(((gap + 6) / 7 * 7) as u64);
            while d <= to {
                out.push(d);
                d = d + Days::new(7);
            }
        }
        Cadence::Month { day } | Cadence::Months { day, .. } => {
            let every = match cadence {
                Cadence::Months { months, .. } => i64::from(months.max(1)),
                _ => 1,
            };
            let mut m = NaiveDate::from_ymd_opt(from.year(), from.month(), 1).expect("valid");
            while m <= to {
                if (month_index(m) - month_index(start)).rem_euclid(every) == 0 {
                    let due = day_in_month(m.year(), m.month(), day);
                    if due >= from && due <= to {
                        out.push(due);
                    }
                }
                m = m + Months::new(1);
            }
        }
    }
    out
}

/// The first due date on or after `on`.
pub fn next_due(cadence: Cadence, start: NaiveDate, on: NaiveDate) -> Option<NaiveDate> {
    occurrences(cadence, start, on, on + Days::new(800)).into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn monthly_bills_move_back_in_short_months() {
        let due = occurrences(Cadence::Month { day: 31 }, d(2026, 1, 1), d(2026, 1, 1), d(2026, 4, 30));
        assert_eq!(due, vec![d(2026, 1, 31), d(2026, 2, 28), d(2026, 3, 31), d(2026, 4, 30)]);
    }

    #[test]
    fn nothing_before_the_bill_started() {
        let due = occurrences(Cadence::Month { day: 1 }, d(2026, 3, 15), d(2026, 1, 1), d(2026, 5, 31));
        assert_eq!(due, vec![d(2026, 4, 1), d(2026, 5, 1)]);
    }

    #[test]
    fn every_six_months_counts_from_the_start() {
        // Semi-annual car insurance first due in March.
        let c = Cadence::Months { months: 6, day: 3 };
        let due = occurrences(c, d(2026, 3, 1), d(2026, 1, 1), d(2027, 12, 31));
        assert_eq!(due, vec![d(2026, 3, 3), d(2026, 9, 3), d(2027, 3, 3), d(2027, 9, 3)]);
        assert_eq!(c.per_year(), 2);
    }

    #[test]
    fn weekly_from_the_start_date() {
        let due = occurrences(Cadence::Week, d(2026, 10, 2), d(2026, 10, 10), d(2026, 10, 31));
        assert_eq!(due, vec![d(2026, 10, 16), d(2026, 10, 23), d(2026, 10, 30)]);
        assert_eq!(occurrences(Cadence::Week, d(2026, 10, 2), d(2026, 10, 9), d(2026, 10, 9)), vec![d(2026, 10, 9)]);
    }

    #[test]
    fn next_due_looks_ahead() {
        assert_eq!(next_due(Cadence::Month { day: 5 }, d(2026, 1, 1), d(2026, 10, 6)), Some(d(2026, 11, 5)));
        assert_eq!(next_due(Cadence::Month { day: 5 }, d(2026, 1, 1), d(2026, 10, 5)), Some(d(2026, 10, 5)));
    }

    #[test]
    fn validation() {
        assert!(Cadence::Month { day: 0 }.validate().is_err());
        assert!(Cadence::Months { months: 0, day: 1 }.validate().is_err());
        assert!(Cadence::Week.validate().is_ok());
    }
}
