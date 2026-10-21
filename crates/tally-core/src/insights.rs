//! Household insights: when a savings goal will be reached, and a budget
//! score that sums up how the household is doing.

use crate::money::Cents;
use chrono::{Datelike, Months, NaiveDate};
use serde::Serialize;

// ---------------------------------------------------------------- goals

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Projection {
    pub remaining: Cents,
    /// 0..=1000, per mille. An overfunded goal reads as complete, not 120%.
    pub permille: u32,
    /// Average put in per month that saw a contribution.
    pub monthly_rate: Cents,
    /// First of the month she gets there at that rate; `None` with no rate yet.
    pub projected: Option<NaiveDate>,
    pub months_remaining: Option<u32>,
    /// With a target date: is the current rate enough?
    pub on_track: Option<bool>,
    /// With a target date: what it takes per month from here.
    pub required_monthly: Option<Cents>,
}

fn months_between(from: NaiveDate, to: NaiveDate) -> i64 {
    i64::from(to.year() - from.year()) * 12 + i64::from(to.month()) - i64::from(from.month())
}

/// When does she get there, at the rate she's actually going?
///
/// Unlike the monthly meters, which extrapolate from part of one month, a
/// goal's rate is averaged over the whole months that saw a contribution, so
/// one generous month doesn't promise Hawaii by Christmas and one skipped
/// month doesn't push it to never.
///
/// `saved` is everything in the goal so far; `contributed` is the part put in
/// month by month, which is what the rate is judged on (money that was
/// already saved when the goal was set up isn't a pace).
pub fn project_goal(target: Cents, saved: Cents, contributed: Cents, months_contributing: u32, target_date: Option<NaiveDate>, today: NaiveDate) -> Projection {
    let remaining = (target - saved).max(Cents::ZERO);
    let permille = if target.0 > 0 { (i128::from(saved.0.max(0)) * 1000 / i128::from(target.0)).min(1000) as u32 } else { 0 };
    let monthly_rate = if months_contributing > 0 { Cents((contributed.0 as f64 / f64::from(months_contributing)).round() as i64) } else { Cents::ZERO };
    let months_until_target = target_date.map(|d| months_between(today, d).max(0));
    let required_monthly = months_until_target.map(|m| if m > 0 { Cents((remaining.0 + m - 1) / m) } else { remaining });
    let this_month = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).expect("valid");

    if remaining.is_zero() {
        return Projection {
            remaining,
            permille,
            monthly_rate,
            projected: Some(this_month),
            months_remaining: Some(0),
            on_track: target_date.map(|_| true),
            required_monthly,
        };
    }
    // A rate of zero has no arrival date; "never" would be true and useless.
    if monthly_rate.0 <= 0 {
        return Projection {
            remaining,
            permille,
            monthly_rate: Cents::ZERO,
            projected: None,
            months_remaining: None,
            on_track: target_date.map(|_| false),
            required_monthly,
        };
    }
    let months = ((remaining.0 + monthly_rate.0 - 1) / monthly_rate.0) as u32;
    Projection {
        remaining,
        permille,
        monthly_rate,
        projected: Some(this_month + Months::new(months)),
        months_remaining: Some(months),
        on_track: months_until_target.map(|m| i64::from(months) <= m),
        required_monthly,
    }
}

// ---------------------------------------------------------------- score

/// One month's household totals, for the score's look back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonthTotals {
    pub income: Cents,
    pub spending: Cents,
    /// What there was to work with, or the planned spending; zero if no plan.
    pub planned: Cents,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoreInput {
    /// Recent months, oldest first; the last is the month being scored.
    pub months: Vec<MonthTotals>,
    /// Bills whose due date has passed this month, and how many of those are unpaid.
    pub bills_due: u32,
    pub bills_unpaid: u32,
    /// Put toward goals or savings this month.
    pub set_aside: Cents,
    pub day_of_month: u32,
    pub days_in_month: u32,
    /// The month's projected spending, when the caller knows better than a
    /// straight line (bills and fixed costs land on their day).
    pub pace: Option<Cents>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Band {
    Poor,
    Fair,
    Good,
    VeryGood,
    Excellent,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScoreFactor {
    pub key: &'static str,
    pub label: &'static str,
    /// Share of the score, out of 100.
    pub weight: u32,
    /// How well it's going, 0..1.
    pub fraction: f64,
    /// What it contributed, out of `weight`.
    pub points: f64,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BudgetScore {
    pub score: u32,
    pub band: Band,
    pub factors: Vec<ScoreFactor>,
}

pub fn band_for(score: u32) -> Band {
    match score {
        ..580 => Band::Poor,
        580..670 => Band::Fair,
        670..740 => Band::Good,
        740..800 => Band::VeryGood,
        _ => Band::Excellent,
    }
}

fn clamp01(x: f64) -> f64 {
    x.clamp(0.0, 1.0)
}

/// Linear 0..1 between two bounds; works with the bounds either way round.
fn scale(value: f64, zero_at: f64, one_at: f64) -> f64 {
    if zero_at == one_at {
        return if value >= one_at { 1.0 } else { 0.0 };
    }
    clamp01((value - zero_at) / (one_at - zero_at))
}

/// Whole dollars with thousands separators, for sentences: `$7,200`.
pub fn dollars(c: Cents) -> String {
    let d = (c.0 as f64 / 100.0).round() as i64;
    let digits = d.abs().to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    format!("{}${out}", if d < 0 { "-" } else { "" })
}

/// A budget score, shaped like a credit score on purpose: a familiar range,
/// a band with a name, and factors to work on. It borrows the shape, not the
/// meaning: it's about her own budget and nothing outside the app.
///
/// Living within what comes in dominates, judged over the whole window so a
/// late check doesn't swing it. Bills come next: those failures have
/// consequences. Pace and saving are real but recoverable. Steady income is
/// last and small, because freelance money arrives when it arrives.
pub fn score(input: &ScoreInput) -> BudgetScore {
    let current = input.months.last().copied().unwrap_or(MonthTotals { income: Cents::ZERO, spending: Cents::ZERO, planned: Cents::ZERO });
    let total_in: i64 = input.months.iter().map(|m| m.income.0).sum();
    let total_out: i64 = input.months.iter().map(|m| m.spending.0.max(0)).sum();
    let n = input.months.len();
    let mut factors = Vec::new();

    let ratio = if total_out == 0 { if total_in > 0 { 2.0 } else { 1.0 } } else { total_in as f64 / total_out as f64 };
    factors.push(ScoreFactor {
        key: "within_income",
        label: "Living within your income",
        weight: 35,
        fraction: scale(ratio, 0.8, 1.15),
        points: 0.0,
        summary: if total_in >= total_out {
            format!("{} more in than out over {n} months", dollars(Cents(total_in - total_out)))
        } else {
            format!("{} more out than in over {n} months", dollars(Cents(total_out - total_in)))
        },
    });

    let paid = input.bills_due.saturating_sub(input.bills_unpaid);
    factors.push(ScoreFactor {
        key: "bills",
        label: "Bills paid on time",
        weight: 25,
        fraction: if input.bills_due == 0 { 1.0 } else { clamp01(f64::from(paid) / f64::from(input.bills_due)) },
        points: 0.0,
        summary: if input.bills_due == 0 { "nothing due yet this month".into() } else { format!("{paid} of {} due so far are paid", input.bills_due) },
    });

    let elapsed = (f64::from(input.day_of_month) / f64::from(input.days_in_month.max(1))).clamp(0.01, 1.0);
    let paced = match input.pace {
        Some(p) => p.0.max(0) as f64,
        None => current.spending.0.max(0) as f64 / elapsed,
    };
    let plan = current.planned.0 as f64;
    factors.push(ScoreFactor {
        key: "pace",
        label: "Spending to plan",
        weight: 20,
        // No plan is an unanswered question, not an overspend: neutral.
        fraction: if plan <= 0.0 { 0.5 } else { scale(paced / plan, 1.25, 0.9) },
        points: 0.0,
        summary: if plan <= 0.0 {
            "no plan for this month".into()
        } else {
            format!("on pace for {} of {}", dollars(Cents(paced.round() as i64)), dollars(current.planned))
        },
    });

    // A tenth of what came in this window, per month, is the full mark.
    let monthly_in = total_in as f64 / n.max(1) as f64;
    factors.push(ScoreFactor {
        key: "aside",
        label: "Putting money aside",
        weight: 10,
        fraction: if monthly_in > 0.0 { clamp01(input.set_aside.0 as f64 / (monthly_in * 0.1)) } else { 0.0 },
        points: 0.0,
        summary: if input.set_aside.0 > 0 { format!("{} set aside this month", dollars(input.set_aside)) } else { "nothing set aside yet this month".into() },
    });

    let incomes: Vec<f64> = input.months.iter().map(|m| m.income.0 as f64).collect();
    let mean = incomes.iter().sum::<f64>() / incomes.len().max(1) as f64;
    let variance = incomes.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / incomes.len().max(1) as f64;
    let cv = if mean > 0.0 { variance.sqrt() / mean } else { 1.0 };
    factors.push(ScoreFactor {
        key: "steady",
        label: "Steady income",
        weight: 10,
        fraction: if incomes.len() < 2 { 0.5 } else { scale(cv, 0.8, 0.15) },
        points: 0.0,
        summary: if incomes.len() < 2 {
            "not enough history yet".into()
        } else if cv < 0.25 {
            "arrives at a fairly even rate".into()
        } else {
            "arrives unevenly, which is worth planning around".into()
        },
    });

    for f in &mut factors {
        f.points = f.fraction * f64::from(f.weight);
    }
    let earned: f64 = factors.iter().map(|f| f.points).sum();
    let score = (300.0 + 550.0 * earned / 100.0).round() as u32;
    BudgetScore { score, band: band_for(score), factors }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn a_goal_projects_from_whole_contributing_months() {
        let p = project_goal(Cents(300_000), Cents(60_000), Cents(60_000), 3, None, d(2026, 10, 20));
        assert_eq!(p.monthly_rate, Cents(20_000));
        assert_eq!(p.months_remaining, Some(12));
        assert_eq!(p.projected, Some(d(2027, 10, 1)));
        assert_eq!(p.permille, 200);
        assert_eq!(p.on_track, None, "no date, so nothing to be behind on");
    }

    #[test]
    fn a_dated_goal_says_what_it_takes() {
        let p = project_goal(Cents(300_000), Cents(60_000), Cents(60_000), 3, Some(d(2027, 4, 1)), d(2026, 10, 20));
        assert_eq!(p.required_monthly, Some(Cents(40_000)));
        assert_eq!(p.on_track, Some(false));
        let fresh = project_goal(Cents(100_000), Cents::ZERO, Cents::ZERO, 0, Some(d(2027, 4, 1)), d(2026, 10, 20));
        assert_eq!((fresh.projected, fresh.on_track), (None, Some(false)));
        assert_eq!(fresh.required_monthly, Some(Cents(16_667)));
    }

    #[test]
    fn money_already_saved_isnt_a_pace() {
        // $400 saved before, then $250 and $200 in two months: $225 a month.
        let p = project_goal(Cents(300_000), Cents(85_000), Cents(45_000), 2, Some(d(2027, 6, 1)), d(2026, 10, 20));
        assert_eq!(p.monthly_rate, Cents(22_500));
        assert_eq!(p.months_remaining, Some(10));
        assert_eq!(p.on_track, Some(false));
    }

    #[test]
    fn a_finished_goal_is_done_not_overdone() {
        let p = project_goal(Cents(100_000), Cents(120_000), Cents(120_000), 2, Some(d(2026, 1, 1)), d(2026, 10, 20));
        assert_eq!((p.permille, p.remaining, p.months_remaining, p.on_track), (1000, Cents::ZERO, Some(0), Some(true)));
    }

    fn month(income: i64, spending: i64, planned: i64) -> MonthTotals {
        MonthTotals { income: Cents(income), spending: Cents(spending), planned: Cents(planned) }
    }

    fn input(months: Vec<MonthTotals>) -> ScoreInput {
        ScoreInput { months, bills_due: 0, bills_unpaid: 0, set_aside: Cents::ZERO, day_of_month: 15, days_in_month: 30, pace: None }
    }

    #[test]
    fn weights_add_up_and_the_score_is_the_sum() {
        let s = score(&input(vec![month(500_000, 400_000, 0); 6]));
        assert_eq!(s.factors.iter().map(|f| f.weight).sum::<u32>(), 100);
        let earned: f64 = s.factors.iter().map(|f| f.points).sum();
        assert_eq!(s.score, (300.0 + 5.5 * earned).round() as u32);
        assert!((300..=850).contains(&s.score));
    }

    #[test]
    fn the_extremes_stay_in_range() {
        let best = ScoreInput { set_aside: Cents(1_000_000), day_of_month: 30, ..input(vec![month(1_000_000, 100_000, 1_000_000); 6]) };
        assert_eq!(score(&best).score, 850);
        assert_eq!(score(&best).band, Band::Excellent);
        let worst = ScoreInput { bills_due: 3, bills_unpaid: 3, day_of_month: 1, ..input(vec![month(100_000, 0, 0), month(0, 900_000, 100_000)]) };
        let w = score(&worst);
        assert!(w.score >= 300 && w.band == Band::Poor, "{}", w.score);
    }

    #[test]
    fn no_plan_is_neutral_not_failing() {
        let s = score(&input(vec![month(500_000, 400_000, 0)]));
        let pace = s.factors.iter().find(|f| f.key == "pace").unwrap();
        assert_eq!(pace.fraction, 0.5);
    }

    #[test]
    fn overspending_over_the_window_costs_the_most() {
        let fine = score(&input(vec![month(500_000, 450_000, 500_000); 6])).score;
        let short = score(&input(vec![month(400_000, 520_000, 500_000); 6])).score;
        assert!(fine - short > 150, "{fine} vs {short}");
        let summary = &score(&input(vec![month(400_000, 520_000, 0); 6])).factors[0].summary;
        assert_eq!(summary, "$7,200 more out than in over 6 months");
    }

    #[test]
    fn bands() {
        assert_eq!(band_for(579), Band::Poor);
        assert_eq!(band_for(580), Band::Fair);
        assert_eq!(band_for(739), Band::Good);
        assert_eq!(band_for(799), Band::VeryGood);
        assert_eq!(band_for(800), Band::Excellent);
    }
}
