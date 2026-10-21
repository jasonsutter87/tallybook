//! Household budgets: is this line on plan, given how far into the month we are?
//!
//! The answer is time-aware. $200 into a $400 grocery budget is fine on the
//! 15th and a fire on the 3rd, so every line projects its end-of-month total
//! from the pace so far. And there are three states, not two: a tolerance
//! band in the middle says variance is normal, which matters most in a year
//! whose costs nobody knows yet.

use crate::money::Cents;
use serde::{Deserialize, Serialize};

/// Which way a budget line is meant to go.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Spend up to this: groceries, gas, an allowance.
    #[default]
    Ceiling,
    /// Get at least this much in: savings, a set-aside, an income goal.
    Floor,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Ceiling => "ceiling",
            Direction::Floor => "floor",
        }
    }

    pub fn parse(s: &str) -> Option<Direction> {
        match s {
            "ceiling" => Some(Direction::Ceiling),
            "floor" => Some(Direction::Floor),
            _ => None,
        }
    }
}

/// Where the line stands against its target. The same math for both
/// directions: "under" always means below target, never "doing well".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Zone {
    Under,
    InRange,
    TrendingOver,
    Over,
}

/// How to feel about that. Coming in under on dining out is a win; coming in
/// under on the car-insurance set-aside means March is an ambush.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Sentiment {
    Good,
    Caution,
    Alarm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusInput {
    pub allocated: Cents,
    /// Progress toward the target as a positive amount: spent for a
    /// ceiling, put in for a floor.
    pub progress: Cents,
    pub direction: Direction,
    /// Half-width of the "in range" band, as a percent of the allocation.
    pub tolerance_pct: u32,
    /// 1-based day of the month being viewed.
    pub day_of_month: u32,
    pub days_in_month: u32,
    /// The part of `progress` that came from scheduled bills. Bills land on
    /// their day, not evenly, so they're counted as-is rather than projected.
    pub scheduled_paid: Cents,
    /// Scheduled bills still to come this month.
    pub scheduled_due: Cents,
}

impl StatusInput {
    pub fn new(allocated: Cents, progress: Cents, direction: Direction, tolerance_pct: u32, day_of_month: u32, days_in_month: u32) -> Self {
        StatusInput { allocated, progress, direction, tolerance_pct, day_of_month, days_in_month, scheduled_paid: Cents::ZERO, scheduled_due: Cents::ZERO }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Status {
    pub zone: Zone,
    pub sentiment: Sentiment,
    /// Projected end-of-month total at the current pace.
    pub pace: Cents,
    /// Fraction of the month elapsed, 0..1.
    pub elapsed: f64,
    /// Allocated minus progress. Negative means past the target.
    pub remaining: Cents,
    /// The tolerance band, in cents.
    pub band: Cents,
}

/// Rounds `n / d` to the nearest integer, halves up (as JavaScript's
/// `Math.round` does, so figures match the app this logic came from).
fn round_div(n: i128, d: i128) -> i64 {
    (2 * n + d).div_euclid(2 * d) as i64
}

pub fn status(input: &StatusInput) -> Status {
    let days = input.days_in_month.max(1);
    let day = input.day_of_month.clamp(1, days);
    let elapsed = f64::from(day) / f64::from(days);

    // Scheduled bills count as they are; only the rest is projected.
    let variable = input.progress - input.scheduled_paid;
    let projected = round_div(i128::from(variable.0) * i128::from(days), i128::from(day));
    let pace = input.scheduled_paid + input.scheduled_due.max(Cents::ZERO) + Cents(projected);

    let band = Cents(round_div(i128::from(input.allocated.0.abs()) * i128::from(input.tolerance_pct), 100));
    let remaining = input.allocated - input.progress;
    let zone = zone(input.allocated, input.progress, pace, band);
    Status { zone, sentiment: sentiment(input.direction, zone), pace, elapsed, remaining, band }
}

fn zone(allocated: Cents, progress: Cents, pace: Cents, band: Cents) -> Zone {
    // Nothing to measure against: any movement at all is over.
    if allocated.0 <= 0 {
        return if progress.0 > 0 { Zone::Over } else { Zone::InRange };
    }
    // Already past the target; that's a fact, not a projection.
    if progress > allocated {
        return Zone::Over;
    }
    if pace > allocated + band {
        return Zone::TrendingOver;
    }
    if pace < allocated - band {
        return Zone::Under;
    }
    Zone::InRange
}

fn sentiment(direction: Direction, zone: Zone) -> Sentiment {
    match direction {
        Direction::Ceiling => match zone {
            Zone::Over => Sentiment::Alarm,
            Zone::TrendingOver => Sentiment::Caution,
            _ => Sentiment::Good,
        },
        // A floor fails by staying empty. Overfunding it is never a problem.
        Direction::Floor => {
            if zone == Zone::Under {
                Sentiment::Alarm
            } else {
                Sentiment::Good
            }
        }
    }
}

/// Share of a ceiling used so far, in whole percent, for alerts like
/// "you've used 80% of your grocery budget". `None` without an allocation.
pub fn percent_used(allocated: Cents, progress: Cents) -> Option<u32> {
    (allocated.0 > 0).then(|| (i128::from(progress.0.max(0)) * 100 / i128::from(allocated.0)) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// $400 ceiling, 10% band, day 15 of 30.
    fn groceries(progress: i64, day: u32) -> StatusInput {
        StatusInput::new(Cents(40_000), Cents(progress), Direction::Ceiling, 10, day, 30)
    }

    #[test]
    fn the_same_spend_is_alarming_early_and_fine_late() {
        assert_eq!(status(&groceries(20_000, 3)).zone, Zone::TrendingOver);
        assert_eq!(status(&groceries(20_000, 15)).zone, Zone::InRange);
        assert_eq!(status(&groceries(20_000, 30)).zone, Zone::Under);
    }

    #[test]
    fn projects_from_the_pace_so_far() {
        // $148 by day 9 of 30 projects to $493.33.
        let s = status(&groceries(14_800, 9));
        assert!((s.elapsed - 0.3).abs() < 1e-9);
        assert_eq!(s.pace, Cents(49_333));
        assert_eq!(s.zone, Zone::TrendingOver);
        assert_eq!(s.sentiment, Sentiment::Caution);
    }

    #[test]
    fn says_what_is_left_even_when_negative() {
        assert_eq!(status(&groceries(15_000, 15)).remaining, Cents(25_000));
        assert_eq!(status(&groceries(45_000, 15)).remaining, Cents(-5_000));
    }

    #[test]
    fn the_band_is_inclusive_and_breaks_one_cent_past() {
        // $40 band on $400: in range spans a $360..$440 pace.
        assert_eq!(status(&groceries(22_000, 15)).zone, Zone::InRange);
        assert_eq!(status(&groceries(18_000, 15)).zone, Zone::InRange);
        assert_eq!(status(&groceries(22_001, 15)).zone, Zone::TrendingOver);
        assert_eq!(status(&groceries(17_999, 15)).zone, Zone::Under);
    }

    #[test]
    fn honours_a_custom_tolerance() {
        let tight = StatusInput { tolerance_pct: 1, ..groceries(21_000, 15) };
        let loose = StatusInput { tolerance_pct: 50, ..groceries(21_000, 15) };
        assert_eq!(status(&tight).zone, Zone::TrendingOver);
        assert_eq!(status(&loose).zone, Zone::InRange);
    }

    #[test]
    fn real_overspend_beats_any_projection() {
        let s = status(&groceries(40_001, 30));
        assert_eq!((s.zone, s.sentiment), (Zone::Over, Sentiment::Alarm));
        assert_eq!(status(&groceries(40_000, 30)).zone, Zone::InRange);
    }

    #[test]
    fn direction_flips_the_feeling_not_the_facts() {
        let ceiling = status(&StatusInput::new(Cents(18_000), Cents(3_000), Direction::Ceiling, 10, 20, 30));
        let floor = status(&StatusInput::new(Cents(18_000), Cents(3_000), Direction::Floor, 10, 20, 30));
        assert_eq!((ceiling.zone, floor.zone), (Zone::Under, Zone::Under));
        assert_eq!((ceiling.sentiment, floor.sentiment), (Sentiment::Good, Sentiment::Alarm));

        let overfunded = status(&StatusInput::new(Cents(18_000), Cents(25_000), Direction::Floor, 10, 15, 30));
        assert_eq!((overfunded.zone, overfunded.sentiment), (Zone::Over, Sentiment::Good));
    }

    #[test]
    fn guards_against_odd_days() {
        let day_zero = status(&groceries(10_000, 0));
        let empty_month = status(&StatusInput { days_in_month: 0, ..groceries(10_000, 15) });
        assert!(day_zero.pace.0 > 0 && empty_month.pace.0 > 0);
        assert_eq!(status(&groceries(20_000, 99)).pace, status(&groceries(20_000, 30)).pace);
    }

    #[test]
    fn an_unbudgeted_line_is_over_once_anything_moves() {
        let spent = StatusInput { allocated: Cents::ZERO, ..groceries(500, 15) };
        assert_eq!(status(&spent).zone, Zone::Over);
        let quiet = StatusInput { allocated: Cents::ZERO, ..groceries(0, 15) };
        assert_eq!(status(&quiet).zone, Zone::InRange);
    }

    #[test]
    fn rent_on_the_first_is_not_a_false_alarm() {
        // $3,200 plan, $1,495 rent paid on the 1st plus $500 of everything
        // else by the 14th. Projecting the rent too would say $4,280.
        let naive = StatusInput::new(Cents(320_000), Cents(199_500), Direction::Ceiling, 10, 14, 30);
        assert_eq!(status(&naive).zone, Zone::TrendingOver);
        let aware = StatusInput { scheduled_paid: Cents(149_500), ..naive };
        let s = status(&aware);
        assert_eq!(s.pace, Cents(149_500 + 107_143));
        assert_eq!(s.zone, Zone::Under);
        // A bill still to come is counted in full.
        let with_due = StatusInput { scheduled_due: Cents(50_000), ..aware };
        assert_eq!(status(&with_due).zone, Zone::InRange);
    }

    #[test]
    fn percent_used_for_alerts() {
        assert_eq!(percent_used(Cents(120_000), Cents(96_000)), Some(80));
        assert_eq!(percent_used(Cents(120_000), Cents(-5)), Some(0));
        assert_eq!(percent_used(Cents::ZERO, Cents(5)), None);
    }
}
