//! When recurring bills fall due.
//!
//! A bill repeats monthly, every few months, yearly, or weekly. Monthly-ish
//! bills land on a day of the month, moved back to the month's last day when
//! the month is short (a bill due on the 31st is due April 30th).

use crate::Cents;
use chrono::{Datelike, Days, Months, NaiveDate};
use serde::{Deserialize, Serialize};
use std::ops::RangeInclusive;

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

/// A repeat payment found in past transactions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Pattern {
    pub cadence: Cadence,
    /// The first time it was seen, which is where the schedule counts from.
    pub start: NaiveDate,
    pub last: NaiveDate,
    /// The typical (median) amount.
    pub amount: Cents,
    /// The amount moves around (power, water): a usage bill.
    pub varies: bool,
    pub times: usize,
}

// What `detect` accepts as a schedule. Bank dates slip for weekends,
// holidays and short months, so each cadence is a window of gaps (in days)
// around the ideal one, not an exact count.

/// 7 days, give or take two.
const WEEK_GAP: RangeInclusive<i64> = 5..=9;
/// The median gap of a weekly payer sits closer to 7.
const WEEK_TYPICAL_GAP: RangeInclusive<i64> = 6..=8;
/// 28 to 31 days, plus a few days' slip either way.
const MONTH_GAP: RangeInclusive<i64> = 24..=38;
/// A monthly payer's median gap; a day tighter at the short end.
const MONTH_TYPICAL_GAP: RangeInclusive<i64> = 25..=38;
/// Two months between payments: one month was skipped.
const SKIPPED_MONTH_GAP: RangeInclusive<i64> = 54..=68;
/// 89 to 92 days, give or take a week.
const QUARTER_GAP: RangeInclusive<i64> = 84..=99;
/// 365 days, give or take about ten.
const YEAR_GAP: RangeInclusive<i64> = 355..=376;

/// A monthly bill may skip this many months and still count.
const MONTHS_SKIPPED_ALLOWED: usize = 1;

// Fewer payments than these and a coincidence looks like a schedule.
// Yearly needs only two: that's all a two-year look back can hold.
const MIN_SEEN_WEEKLY: usize = 4;
const MIN_SEEN_MONTHLY: usize = 3;
const MIN_SEEN_QUARTERLY: usize = 3;
/// A usage bill's amount says little, so it needs more payments to trust.
const MIN_SEEN_TO_VARY: usize = 4;

// Days since the last payment after which the schedule has stopped (a
// cancelled gym, a closed account): the usual gap plus some grace.
const WEEKLY_STOPS_AFTER_DAYS: i64 = 15; // two weeks and a day
const MONTHLY_STOPS_AFTER_DAYS: i64 = 50; // a month and about three weeks
const QUARTERLY_STOPS_AFTER_DAYS: i64 = 110; // a quarter and about three weeks
const YEARLY_STOPS_AFTER_DAYS: i64 = 395; // a year and a month

/// Paydays on the 30th, 31st and 1st are a few days apart, not a month.
const MAX_DAY_SPREAD: u32 = 7;

/// A steady bill: within 10% of the typical amount...
const STEADY_PERCENT: i64 = 10;
/// ...or $5, since 10% of a small bill is less than tax or a fee changing.
const STEADY_FLOOR: Cents = Cents(5_00);
/// A usage bill (power, water) moves with the season, but within 40%.
const USAGE_PERCENT: i64 = 40;

// Amounts come from bank imports, so they can be anywhere in i64: the
// percentage maths is done in i128, where it can't overflow and panic.

/// Within 10%, or $5 for small amounts: the same bill.
fn steady(typical: Cents, x: Cents) -> bool {
    let (t, x) = (i128::from(typical.0), i128::from(x.0));
    (x - t).abs() <= (t.abs() * i128::from(STEADY_PERCENT) / 100).max(i128::from(STEADY_FLOOR.0))
}

/// Within 40%: a usage bill that moves with the season.
fn roughly(typical: Cents, x: Cents) -> bool {
    let (t, x) = (i128::from(typical.0), i128::from(x.0));
    (x - t).abs() * 100 <= t.abs() * i128::from(USAGE_PERCENT)
}

/// The middle value; `None` for nothing at all.
fn median<T: Copy + Ord>(xs: &[T]) -> Option<T> {
    let mut v = xs.to_vec();
    v.sort_unstable();
    v.get(v.len() / 2).copied()
}

/// The typical day of the month, treating the 30th, 31st and 1st as close
/// together. `None` when the days are spread more than a week apart.
fn usual_day(dates: &[NaiveDate]) -> Option<u32> {
    let spread = |v: &[u32]| v.iter().max().zip(v.iter().min()).map_or(0, |(hi, lo)| hi - lo);
    let days: Vec<u32> = dates.iter().map(|d| d.day()).collect();
    // Early days pushed past the month's end, so a bill drifting from the 30th to the 1st stays together.
    let wrapped: Vec<u32> = days.iter().map(|&d| if d <= 15 { d + 31 } else { d }).collect();
    let v = if spread(&wrapped) < spread(&days) { wrapped } else { days };
    if spread(&v) > MAX_DAY_SPREAD {
        return None;
    }
    let day = median(&v)?;
    Some(if day > 31 { day - 31 } else { day })
}

/// Look for a schedule in when, and how much, something was paid: weekly,
/// monthly, quarterly, or yearly, and still going as of `today`. A monthly
/// bill may skip a month, and one whose amount moves around (power, water)
/// is still found, marked `varies`.
pub fn detect(seen: &[(NaiveDate, Cents)], today: NaiveDate) -> Option<Pattern> {
    let mut seen = seen.to_vec();
    seen.sort();
    seen.dedup_by_key(|s| s.0);
    // It takes two payments to have a gap at all.
    if seen.len() < 2 {
        return None;
    }
    let dates: Vec<NaiveDate> = seen.iter().map(|s| s.0).collect();
    let gaps: Vec<i64> = dates.windows(2).map(|w| (w[1] - w[0]).num_days()).collect();
    let all_within = |range: &RangeInclusive<i64>| gaps.iter().all(|g| range.contains(g));
    let n = seen.len();
    let day = || usual_day(&dates);
    let typical_gap = median(&gaps)?;

    // (cadence, days without a payment before it's stopped, may the amount vary)
    // Only monthly bills may vary: that's how power and water are billed.
    let (cadence, stops_after, may_vary) = if WEEK_TYPICAL_GAP.contains(&typical_gap) {
        if n < MIN_SEEN_WEEKLY || !all_within(&WEEK_GAP) {
            return None;
        }
        (Cadence::Week, WEEKLY_STOPS_AFTER_DAYS, false)
    } else if MONTH_TYPICAL_GAP.contains(&typical_gap) {
        if n < MIN_SEEN_MONTHLY {
            return None;
        }
        let skipped = gaps.iter().filter(|g| SKIPPED_MONTH_GAP.contains(*g)).count();
        if skipped > MONTHS_SKIPPED_ALLOWED || gaps.iter().any(|g| !MONTH_GAP.contains(g) && !SKIPPED_MONTH_GAP.contains(g)) {
            return None;
        }
        (Cadence::Month { day: day()? }, MONTHLY_STOPS_AFTER_DAYS, true)
    } else if QUARTER_GAP.contains(&typical_gap) {
        if n < MIN_SEEN_QUARTERLY || !all_within(&QUARTER_GAP) {
            return None;
        }
        (Cadence::Months { months: 3, day: day()? }, QUARTERLY_STOPS_AFTER_DAYS, false)
    } else if YEAR_GAP.contains(&typical_gap) {
        if !all_within(&YEAR_GAP) {
            return None;
        }
        (Cadence::Months { months: 12, day: day()? }, YEARLY_STOPS_AFTER_DAYS, false)
    } else {
        return None;
    };
    let last = dates[n - 1];
    if (today - last).num_days() > stops_after {
        return None;
    }
    let amounts: Vec<Cents> = seen.iter().map(|s| s.1).collect();
    let amount = median(&amounts)?;
    let varies = !amounts.iter().all(|a| steady(amount, *a));
    if varies && !(may_vary && n >= MIN_SEEN_TO_VARY && amounts.iter().all(|a| roughly(amount, *a))) {
        return None;
    }
    Some(Pattern { cadence, start: dates[0], last, amount, varies, times: n })
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

    fn c(dollars: i64) -> Cents {
        Cents(dollars * 100)
    }

    #[test]
    fn finds_a_monthly_bill_even_when_the_day_drifts() {
        let seen = [(d(2026, 6, 30), c(160)), (d(2026, 8, 1), c(160)), (d(2026, 8, 31), c(158)), (d(2026, 9, 30), c(160))];
        let p = detect(&seen, d(2026, 10, 21)).unwrap();
        assert_eq!((p.cadence, p.amount, p.varies, p.times), (Cadence::Month { day: 31 }, c(160), false, 4));
        assert_eq!(p.start, d(2026, 6, 30));
    }

    #[test]
    fn one_skipped_month_is_fine() {
        let seen = [(d(2026, 5, 3), c(40)), (d(2026, 6, 3), c(40)), (d(2026, 8, 4), c(40)), (d(2026, 9, 3), c(40))];
        assert_eq!(detect(&seen, d(2026, 9, 20)).unwrap().cadence, Cadence::Month { day: 3 });
    }

    #[test]
    fn usage_bills_vary_but_still_count() {
        let seen = [(d(2026, 6, 20), c(120)), (d(2026, 7, 20), c(160)), (d(2026, 8, 21), c(180)), (d(2026, 9, 19), c(140))];
        let p = detect(&seen, d(2026, 10, 1)).unwrap();
        assert!(p.varies);
        assert_eq!(p.amount, c(160));
        // Too wild to be one bill, or too few to tell.
        assert_eq!(detect(&[(d(2026, 6, 20), c(20)), (d(2026, 7, 20), c(160)), (d(2026, 8, 21), c(90)), (d(2026, 9, 19), c(140))], d(2026, 10, 1)), None);
        assert_eq!(detect(&seen[1..], d(2026, 10, 1)), None);
    }

    #[test]
    fn finds_weekly_quarterly_and_yearly_bills() {
        let weekly: Vec<_> = (0..5).map(|i| (d(2026, 9, 4) + Days::new(i * 7), c(10))).collect();
        assert_eq!(detect(&weekly, d(2026, 10, 6)).unwrap().cadence, Cadence::Week);
        let quarterly = [(d(2026, 1, 15), c(300)), (d(2026, 4, 15), c(300)), (d(2026, 7, 15), c(300))];
        assert_eq!(detect(&quarterly, d(2026, 9, 1)).unwrap().cadence, Cadence::Months { months: 3, day: 15 });
        let yearly = [(d(2025, 3, 2), c(139)), (d(2026, 3, 2), c(139))];
        assert_eq!(detect(&yearly, d(2026, 10, 1)).unwrap().cadence, Cadence::Months { months: 12, day: 2 });
    }

    #[test]
    fn stopped_scattered_or_one_off_payments_are_not_bills() {
        // Stopped: nothing since June.
        let stopped = [(d(2026, 4, 3), c(15)), (d(2026, 5, 3), c(15)), (d(2026, 6, 3), c(15))];
        assert_eq!(detect(&stopped, d(2026, 10, 1)), None);
        // Monthly-ish gaps but all over the month.
        let random = [(d(2026, 6, 2), c(15)), (d(2026, 6, 29), c(15)), (d(2026, 8, 1), c(15)), (d(2026, 8, 20), c(15))];
        assert_eq!(detect(&random, d(2026, 9, 1)), None);
        assert_eq!(detect(&[(d(2026, 9, 1), c(15))], d(2026, 9, 2)), None);
    }

    #[test]
    fn hostile_amounts_dont_panic() {
        // Amounts at the edges of i64, as a malicious import could carry.
        let at = |amount: i64| [(d(2026, 6, 3), Cents(amount)), (d(2026, 7, 3), Cents(amount)), (d(2026, 8, 3), Cents(amount)), (d(2026, 9, 3), Cents(amount))];
        for amount in [i64::MAX, i64::MIN, i64::MIN + 1, i64::MAX / 10 + 1] {
            assert_eq!(detect(&at(amount), d(2026, 9, 20)).unwrap().amount, Cents(amount));
        }
        let wild = [(d(2026, 6, 3), Cents(i64::MAX)), (d(2026, 7, 3), Cents(1)), (d(2026, 8, 3), Cents(i64::MIN)), (d(2026, 9, 3), Cents(i64::MAX))];
        assert_eq!(detect(&wild, d(2026, 9, 20)), None);
        assert!(!steady(Cents(i64::MIN), Cents(i64::MAX)));
        assert!(!roughly(Cents(i64::MAX), Cents(i64::MIN)));
        assert_eq!(detect(&[], d(2026, 9, 20)), None);
        assert_eq!(median::<i64>(&[]), None);
    }

    #[test]
    fn validation() {
        assert!(Cadence::Month { day: 0 }.validate().is_err());
        assert!(Cadence::Months { months: 0, day: 1 }.validate().is_err());
        assert!(Cadence::Week.validate().is_ok());
    }
}
