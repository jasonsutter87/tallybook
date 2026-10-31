//! Goals, and what the household's numbers add up to: alerts worth
//! interrupting for, where the room to save is, a budget score, and wins
//! worth celebrating.
//!
//! Everything here reads the household side only, and nothing is stored but
//! goals: alerts, opportunities, the score and wins are worked out fresh from
//! the books each time they're asked for. The pure arithmetic (goal
//! projections, the score's weights) lives in `tally_core::insights`.

use crate::{insert_with_id, BudgetTarget, DueState, Result, Store, StoreError, check_entry_not_locked, month_of, month_range, rule};
use chrono::{Months, NaiveDate};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tally_core::budget::{Direction, Sentiment, Zone};
use tally_core::insights::{self, BudgetScore, MonthTotals, Projection, ScoreInput};
use tally_core::journal::EntryId;
use tally_core::{AccountId, AccountKind, AccountSubtype, Cents, EntryLine, Flex, NewEntry, Scope};

pub type GoalId = i64;

/// A budget line at or past this percent of its ceiling gets a heads-up.
const CAUTION_PERCENT: u32 = 80;
/// A floor (savings, income) isn't called behind before this day of the
/// month: early on, most of it simply hasn't come in yet.
const FLOOR_ALERT_FROM_DAY: u32 = 20;
/// Unpaid bills are looked for this far back: a month, so last month's
/// stragglers still show.
const OVERDUE_LOOKBACK_DAYS: u64 = 31;
/// "Usual" spending or income is judged on this many months before.
const USUAL_MONTHS: u32 = 3;
/// The budget score looks at this many months, the asked-for one included.
const SCORE_MONTHS: u32 = 6;
/// Opportunities show what trimming your-choice spending by 1/N would free.
const LIFESTYLE_TRIM_DIVISOR: i64 = 5;

/// A savings goal as the user sets it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewGoal {
    pub name: String,
    /// The amount to reach; always positive.
    pub target: Cents,
    #[serde(default)]
    pub target_date: Option<NaiveDate>,
    /// The household account (savings, usually) the money is kept in.
    pub account_id: AccountId,
    /// What was already saved before the goal was set up; it counts toward
    /// the target but not toward the monthly pace.
    #[serde(default)]
    pub starting: Cents,
    #[serde(default)]
    pub note: String,
}

/// A goal with where it stands today.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Goal {
    pub id: GoalId,
    #[serde(flatten)]
    pub goal: NewGoal,
    /// `YYYY-MM-DD` the running total reached the target and stayed there.
    pub achieved_at: Option<String>,
    pub archived: bool,
    /// `starting` plus every contribution, money taken back out included.
    pub saved: Cents,
    /// How many entries are tagged with the goal.
    pub contributions: i64,
    #[serde(flatten)]
    pub projection: Projection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Contributed {
    pub entry_id: EntryId,
    /// This contribution is the one that reached the target.
    pub finished: bool,
}

/// How loudly an alert speaks; alerts are sorted loudest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    Info,
    Caution,
    Alarm,
}

/// Something worth interrupting the user for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Alert {
    pub tone: Tone,
    pub title: String,
    pub body: String,
    /// The budget line or bill it's about, for linking.
    pub target: Option<BudgetTarget>,
    pub bill_id: Option<i64>,
}

/// Spending in one flexibility tier (fixed, usage, lifestyle, or not set).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FlexTier {
    /// `None` is categories nobody has said how far they bend.
    pub flex: Option<Flex>,
    pub spending: Cents,
    /// Average of the `USUAL_MONTHS` before.
    pub usual: Cents,
}

/// One category that can bend: this month against its usual.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CategoryGap {
    pub account_id: AccountId,
    pub name: String,
    pub flex: Option<Flex>,
    pub spending: Cents,
    pub usual: Cents,
}

/// Where the give is: spending split by how far it can bend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Opportunities {
    pub month: String,
    pub income: Cents,
    pub spending: Cents,
    /// Income minus spending; negative is the gap to close.
    pub gap: Cents,
    pub tiers: Vec<FlexTier>,
    /// Categories that bend, biggest first.
    pub categories: Vec<CategoryGap>,
    /// Trimming your-choice spending by a fifth (`LIFESTYLE_TRIM_DIVISOR`)
    /// would free up this much.
    pub lifestyle_fifth: Cents,
}

/// A piece of good news.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Win {
    /// What sort of win, for the UI to pick an icon: `under_budget`, `ahead`,
    /// `gap`, `saved`, `goal` or `new_income`.
    pub kind: &'static str,
    pub title: String,
    pub body: String,
}

fn goal_from_row(r: &Row) -> rusqlite::Result<(GoalId, NewGoal, Option<String>, bool)> {
    Ok((
        r.get(0)?,
        NewGoal {
            name: r.get(1)?,
            target: Cents(r.get(2)?),
            target_date: r.get(3)?,
            account_id: r.get(4)?,
            starting: Cents(r.get(5)?),
            note: r.get(6)?,
        },
        r.get(7)?,
        r.get(8)?,
    ))
}

/// Amounts in sentences are always positive; the words say which way.
fn money(c: Cents) -> String {
    insights::dollars(c.abs())
}

/// "March" for `2026-03`; the raw text if it isn't a month.
fn month_name(month: &str) -> String {
    month_range(month).map(|(from, _)| from.format("%B").to_string()).unwrap_or_else(|_| month.to_string())
}

/// The month `back` months before `month`.
fn prev_month(month: &str, back: u32) -> Result<String> {
    Ok(month_of(month_range(month)?.0 - Months::new(back)))
}

impl Store {
    // ------------------------------------------------------------ goals

    fn validate_goal(&self, g: &NewGoal) -> Result<()> {
        if g.name.trim().is_empty() {
            return rule("give the goal a name");
        }
        if g.target.0 <= 0 {
            return rule("a goal needs an amount to reach");
        }
        if g.starting.0 < 0 {
            return rule("what's already saved can't be negative");
        }
        let a = self.account(g.account_id)?;
        if a.scope != Scope::Household || a.kind() != AccountKind::Asset {
            return rule("keep a goal's money in a household account, like savings");
        }
        Ok(())
    }

    /// Starts a goal. Money reaches it by `contribute_to_goal` or by tagging
    /// a transfer with `set_entry_goal`.
    pub fn create_goal(&self, g: &NewGoal) -> Result<GoalId> {
        self.validate_goal(g)?;
        let id = insert_with_id(
            &self.conn,
            "goals",
            "INSERT INTO goals(name, target, target_date, account_id, starting, note, id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![g.name.trim(), g.target.0, g.target_date, g.account_id, g.starting.0, g.note.trim()],
        )?;
        Ok(id)
    }

    pub fn update_goal(&self, id: GoalId, g: &NewGoal) -> Result<()> {
        self.validate_goal(g)?;
        let n = self.conn.execute(
            "UPDATE goals SET name = ?1, target = ?2, target_date = ?3, account_id = ?4, starting = ?5, note = ?6 WHERE id = ?7",
            params![g.name.trim(), g.target.0, g.target_date, g.account_id, g.starting.0, g.note.trim(), id],
        )?;
        if n == 0 {
            return Err(StoreError::NotFound("goal", id));
        }
        self.refresh_achieved(id)?;
        Ok(())
    }

    /// Archived goals keep their history and their money; they just stop
    /// showing as in progress.
    pub fn set_goal_archived(&self, id: GoalId, archived: bool) -> Result<()> {
        if self.conn.execute("UPDATE goals SET archived = ?1 WHERE id = ?2", params![archived, id])? == 0 {
            return Err(StoreError::NotFound("goal", id));
        }
        Ok(())
    }

    /// Money put toward each goal, per month: (goal, "YYYY-MM", amount).
    fn goal_contributions(&self) -> Result<Vec<(GoalId, String, Cents)>> {
        let mut stmt = self.conn.prepare(
            "SELECT e.goal_id, substr(e.date, 1, 7), SUM(l.amount)
             FROM journal_entries e
             JOIN goals g ON g.id = e.goal_id
             JOIN journal_lines l ON l.entry_id = e.id AND l.account_id = g.account_id
             WHERE e.voided_by_id IS NULL AND e.reverses_id IS NULL
             GROUP BY e.goal_id, substr(e.date, 1, 7)",
        )?;
        Ok(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, Cents(r.get(2)?))))?.collect::<rusqlite::Result<_>>()?)
    }

    /// Every goal, in progress first, each projected as of `today`.
    pub fn goals(&self, today: NaiveDate) -> Result<Vec<Goal>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, target, target_date, account_id, starting, note, achieved_at, archived FROM goals ORDER BY archived, achieved_at IS NOT NULL, id",
        )?;
        let rows = stmt.query_map([], goal_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        let contributions = self.goal_contributions()?;
        let counts: HashMap<GoalId, i64> = {
            let mut stmt = self.conn.prepare("SELECT goal_id, COUNT(*) FROM journal_entries WHERE goal_id IS NOT NULL AND voided_by_id IS NULL AND reverses_id IS NULL GROUP BY goal_id")?;
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
        };
        Ok(rows
            .into_iter()
            .map(|(id, goal, achieved_at, archived)| {
                let mine: Vec<&(GoalId, String, Cents)> = contributions.iter().filter(|c| c.0 == id).collect();
                let contributed = mine.iter().map(|c| c.2).sum::<Cents>();
                let saved = goal.starting + contributed;
                let months = mine.iter().filter(|c| c.2.0 > 0).count() as u32;
                let projection = insights::project_goal(goal.target, saved, contributed, months, goal.target_date, today);
                Goal { id, goal, achieved_at, archived, saved, contributions: counts.get(&id).copied().unwrap_or(0), projection }
            })
            .collect())
    }

    /// Brings a goal's achieved date in line after its entries change outside
    /// [`Store::contribute_to_goal`] (an import, say).
    pub fn refresh_goal(&self, id: GoalId) -> Result<()> {
        self.refresh_achieved(id).map(|_| ())
    }

    /// Sets the achieved date to the day the contributions first reached
    /// the target (or clears it). True when it went from not reached to reached.
    fn refresh_achieved(&self, id: GoalId) -> Result<bool> {
        let (target, starting, was): (i64, i64, Option<String>) = self
            .conn
            .query_row("SELECT target, starting, achieved_at FROM goals WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?
            .ok_or(StoreError::NotFound("goal", id))?;
        let mut stmt = self.conn.prepare(
            "SELECT e.date, SUM(l.amount)
             FROM journal_entries e
             JOIN goals g ON g.id = e.goal_id
             JOIN journal_lines l ON l.entry_id = e.id AND l.account_id = g.account_id
             WHERE e.goal_id = ?1 AND e.voided_by_id IS NULL AND e.reverses_id IS NULL
             GROUP BY e.id ORDER BY e.date, e.id",
        )?;
        let rows = stmt.query_map([id], |r| Ok((r.get::<_, NaiveDate>(0)?, r.get::<_, i64>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        // Reached once the running total gets there and stays there.
        let mut total = starting;
        let mut reached: Option<NaiveDate> = None;
        for (date, amount) in rows {
            total += amount;
            if total >= target {
                reached.get_or_insert(date);
            } else {
                reached = None;
            }
        }
        let now = reached.map(|d| d.to_string());
        if now != was {
            self.conn.execute("UPDATE goals SET achieved_at = ?1 WHERE id = ?2", params![now, id])?;
        }
        Ok(was.is_none() && now.is_some())
    }

    /// Moves money into the goal's account and tags it. A negative amount
    /// takes money back out.
    pub fn contribute_to_goal(&mut self, id: GoalId, from: AccountId, amount: Cents, date: NaiveDate) -> Result<Contributed> {
        let goal = self.goals(date)?.into_iter().find(|g| g.id == id).ok_or(StoreError::NotFound("goal", id))?;
        if amount.is_zero() {
            return rule("enter an amount");
        }
        if from == goal.goal.account_id {
            return rule("choose where the money comes from");
        }
        let src = self.account(from)?;
        if !matches!(src.subtype, AccountSubtype::Bank | AccountSubtype::CreditCard | AccountSubtype::OtherCurrentAsset) {
            return rule(format!("{} can't move money", src.name));
        }
        let memo = if amount.0 > 0 { format!("Toward {}", goal.goal.name) } else { format!("From {}", goal.goal.name) };
        let entry = NewEntry::new(date, memo)
            .line(EntryLine { account_id: goal.goal.account_id, amount, memo: None })
            .line(EntryLine { account_id: from, amount: -amount, memo: None });
        let entry_id = self.post_entry(&entry)?;
        self.conn.execute("UPDATE journal_entries SET goal_id = ?1 WHERE id = ?2", params![id, entry_id])?;
        Ok(Contributed { entry_id, finished: self.refresh_achieved(id)? })
    }

    /// Counts an existing transfer toward a goal (or stops counting it).
    pub fn set_entry_goal(&self, entry_id: EntryId, goal: Option<GoalId>) -> Result<()> {
        let before: Option<Option<GoalId>> =
            self.conn.query_row("SELECT goal_id FROM journal_entries WHERE id = ?1", [entry_id], |r| r.get(0)).optional()?;
        let Some(before) = before else { return Err(StoreError::NotFound("entry", entry_id)) };
        check_entry_not_locked(&self.conn, entry_id)?;
        if let Some(g) = goal {
            let account: AccountId = self
                .conn
                .query_row("SELECT account_id FROM goals WHERE id = ?1", [g], |r| r.get(0))
                .optional()?
                .ok_or(StoreError::NotFound("goal", g))?;
            if !self.entry(entry_id)?.lines.iter().any(|l| l.account_id == account) {
                return rule("that entry doesn't move money in or out of the goal's account");
            }
        }
        self.conn.execute("UPDATE journal_entries SET goal_id = ?1 WHERE id = ?2", params![goal, entry_id])?;
        for g in [before, goal].into_iter().flatten() {
            self.refresh_achieved(g)?;
        }
        Ok(())
    }

    // ------------------------------------------------------------ alerts

    /// What's worth the user's attention in `month` as of `today`, most urgent first.
    pub fn alerts(&self, month: &str, today: NaiveDate) -> Result<Vec<Alert>> {
        let m = self.month_budget(month, today)?;
        let mut out = Vec::new();
        if m.is_current {
            let days_left = m.days_in_month - m.day_of_month;
            for l in &m.lines {
                let target = Some(l.target);
                match (l.direction, l.percent_used, l.status.zone) {
                    (Direction::Ceiling, Some(p), _) if p > 100 => out.push(Alert {
                        tone: Tone::Alarm,
                        title: format!("{} is over budget", l.name),
                        body: format!("{} over the {} plan, with {days_left} days to go.", money(l.progress - l.amount), money(l.amount)),
                        target,
                        bill_id: None,
                    }),
                    // A fixed category (rent) lands all at once: 100% used on the 1st is expected.
                    (Direction::Ceiling, Some(p), _) if p >= CAUTION_PERCENT && l.amount.0 > 0 && l.flex != Some(Flex::Fixed) => out.push(Alert {
                        tone: Tone::Caution,
                        title: format!("You've used {p}% of your {} budget", l.name),
                        body: format!("{} left for the next {days_left} days.", money(l.status.remaining)),
                        target,
                        bill_id: None,
                    }),
                    (Direction::Ceiling, _, Zone::TrendingOver) => out.push(Alert {
                        tone: Tone::Caution,
                        title: format!("{} is running hot", l.name),
                        body: format!("On pace for {} against a {} plan.", money(l.status.pace), money(l.amount)),
                        target,
                        bill_id: None,
                    }),
                    (Direction::Floor, _, _) if l.status.sentiment == Sentiment::Alarm && m.day_of_month >= FLOOR_ALERT_FROM_DAY => out.push(Alert {
                        tone: Tone::Caution,
                        title: format!("{} is behind", l.name),
                        body: format!("{} to go, with {days_left} days left.", money(l.status.remaining)),
                        target,
                        bill_id: None,
                    }),
                    _ => {}
                }
            }
        }
        for d in self.due_dates(today - chrono::Days::new(OVERDUE_LOOKBACK_DAYS), today, today)? {
            match d.state {
                DueState::Overdue => out.push(Alert {
                    tone: Tone::Alarm,
                    title: format!("{} looks unpaid", d.name),
                    body: format!("It was due {}. Mark it paid if it was paid another way.", d.due_date.format("%B %-d")),
                    target: None,
                    bill_id: Some(d.bill_id),
                }),
                DueState::DueToday => out.push(Alert {
                    tone: Tone::Info,
                    title: format!("{} is due today", d.name),
                    body: if d.amount.is_zero() { "The amount varies.".into() } else { format!("{}.", money(d.amount)) },
                    target: None,
                    bill_id: Some(d.bill_id),
                }),
                _ => {}
            }
        }
        out.sort_by_key(|a| match a.tone {
            Tone::Alarm => 0,
            Tone::Caution => 1,
            Tone::Info => 2,
        });
        Ok(out)
    }

    // ------------------------------------------------------------ opportunity gaps

    /// Household totals in `month` per account of one kind, natural sign.
    fn totals_by_account(&self, month: &str, kind: AccountKind) -> Result<HashMap<AccountId, Cents>> {
        let (from, to) = month_range(month)?;
        let kinds: HashMap<AccountId, AccountKind> = self.accounts_in(Some(Scope::Household))?.into_iter().map(|a| (a.id, a.kind())).collect();
        let mut out: HashMap<AccountId, Cents> = HashMap::new();
        for (account, _, amount, _) in self.household_movement(from, to)? {
            if kinds.get(&account) == Some(&kind) {
                *out.entry(account).or_default() += amount;
            }
        }
        Ok(out)
    }

    fn spending_by_account(&self, month: &str) -> Result<HashMap<AccountId, Cents>> {
        self.totals_by_account(month, AccountKind::Expense)
    }

    fn income_by_account(&self, month: &str) -> Result<HashMap<AccountId, Cents>> {
        self.totals_by_account(month, AccountKind::Income)
    }

    /// Where the room to save is in `month`: spending by how far it can
    /// bend, against the months before.
    pub fn opportunities(&self, month: &str) -> Result<Opportunities> {
        let accounts: HashMap<AccountId, tally_core::Account> = self.accounts_in(Some(Scope::Household))?.into_iter().map(|a| (a.id, a)).collect();
        let now = self.spending_by_account(month)?;
        // "Usual" is the average of the `USUAL_MONTHS` before, counting only
        // months with any household spending, so a new book isn't averaged
        // against months it never saw.
        let mut before: HashMap<AccountId, Cents> = HashMap::new();
        let mut active_months = 0;
        for back in 1..=USUAL_MONTHS {
            let m = self.spending_by_account(&prev_month(month, back)?)?;
            if m.values().any(|c| !c.is_zero()) {
                active_months += 1;
            }
            for (a, c) in m {
                *before.entry(a).or_default() += c;
            }
        }
        let usual = |a: &AccountId| Cents(before.get(a).copied().unwrap_or_default().0 / i64::from(active_months).max(1));
        let income: Cents = self.income_by_account(month)?.values().copied().sum();
        let spending: Cents = now.values().copied().sum();

        let mut tiers: Vec<FlexTier> = [Some(Flex::Fixed), Some(Flex::Usage), Some(Flex::Lifestyle), None]
            .into_iter()
            .map(|flex| FlexTier { flex, spending: Cents::ZERO, usual: Cents::ZERO })
            .collect();
        let ids: HashSet<AccountId> = now.keys().chain(before.keys()).copied().collect();
        let mut categories = Vec::new();
        for id in ids {
            let Some(a) = accounts.get(&id) else { continue };
            let spent = now.get(&id).copied().unwrap_or_default();
            let tier = tiers.iter_mut().find(|t| t.flex == a.flex).expect("every flex has a tier");
            tier.spending += spent;
            tier.usual += usual(&id);
            if a.flex != Some(Flex::Fixed) && (spent.0 > 0 || usual(&id).0 > 0) {
                categories.push(CategoryGap { account_id: id, name: a.name.clone(), flex: a.flex, spending: spent, usual: usual(&id) });
            }
        }
        categories.sort_by(|a, b| b.spending.cmp(&a.spending).then_with(|| a.name.cmp(&b.name)));
        let lifestyle = tiers.iter().find(|t| t.flex == Some(Flex::Lifestyle)).map(|t| t.spending).unwrap_or_default();
        Ok(Opportunities {
            month: month.to_string(),
            income,
            spending,
            gap: income - spending,
            tiers,
            categories,
            lifestyle_fifth: Cents(lifestyle.0.max(0) / LIFESTYLE_TRIM_DIVISOR),
        })
    }

    // ------------------------------------------------------------ score

    /// Money put toward goals in a month, plus what reached savings floors.
    fn set_aside(&self, month: &str, today: NaiveDate) -> Result<Cents> {
        let goals: Cents = self.goal_contributions()?.into_iter().filter(|c| c.1 == month && c.2.0 > 0).map(|c| c.2).sum();
        let floors: Cents = self
            .month_budget(month, today)?
            .lines
            .iter()
            .filter(|l| l.direction == Direction::Floor && matches!(l.target, BudgetTarget::Account(a) if self.account(a).is_ok_and(|x| x.kind() == AccountKind::Asset)))
            .map(|l| l.progress.max(Cents::ZERO))
            .sum();
        Ok(goals.max(floors))
    }

    /// A credit-score-shaped score for how `month` is going, judged on it and
    /// the months before (see `tally_core::insights::score` for the weights).
    pub fn budget_score(&self, month: &str, today: NaiveDate) -> Result<BudgetScore> {
        let mut months = Vec::new();
        for back in (0..SCORE_MONTHS).rev() {
            let m = prev_month(month, back)?;
            let b = self.month_budget(&m, today)?;
            let planned = b.available.unwrap_or(b.planned_for_categories + b.planned_for_people);
            // Skip the empty months before the book began.
            if b.income.is_zero() && b.spending.is_zero() && back > 0 && months.is_empty() {
                continue;
            }
            months.push(MonthTotals { income: b.income, spending: b.spending, planned });
        }
        let current = self.month_budget(month, today)?;
        let (from, to) = month_range(month)?;
        let cutoff = to.min(today);
        let dues: Vec<_> = self.due_dates(from, to, today)?.into_iter().filter(|d| d.due_date <= cutoff && d.state != DueState::Skipped).collect();
        let unpaid = dues.iter().filter(|d| d.state == DueState::Overdue).count() as u32;
        Ok(insights::score(&ScoreInput {
            months,
            bills_due: dues.len() as u32,
            bills_unpaid: unpaid,
            set_aside: self.set_aside(month, today)?,
            day_of_month: current.day_of_month,
            days_in_month: current.days_in_month,
            pace: current.overall.map(|o| o.pace),
        }))
    }

    // ------------------------------------------------------------ wins

    /// Good news worth saying out loud: positive reinforcement, never invented.
    pub fn wins(&self, month: &str, today: NaiveDate) -> Result<Vec<Win>> {
        let mut out = Vec::new();
        let last = prev_month(month, 1)?;
        let last_name = month_name(&last);

        // Last month's your-choice budgets that came in under.
        let lm = self.month_budget(&last, today)?;
        for l in lm.lines.iter().filter(|l| l.direction == Direction::Ceiling && l.flex == Some(Flex::Lifestyle) && l.amount.0 > 0) {
            if l.progress < l.amount && l.progress.0 >= 0 {
                out.push(Win {
                    kind: "under_budget",
                    title: format!("{} under on {}", money(l.amount - l.progress), l.name),
                    body: format!("In {last_name} you spent {} of a {} plan.", money(l.progress), money(l.amount)),
                });
            }
        }
        if lm.income > lm.spending && !lm.spending.is_zero() {
            out.push(Win {
                kind: "ahead",
                title: format!("{last_name} ended {} ahead", money(lm.income - lm.spending)),
                body: "More came in than went out.".into(),
            });
        }

        // Your-choice spending below what's usual: closing the gap.
        let opp = self.opportunities(&last)?;
        if let Some(t) = opp.tiers.iter().find(|t| t.flex == Some(Flex::Lifestyle))
            && t.usual.0 > 0
            && t.spending < t.usual
        {
            out.push(Win {
                kind: "gap",
                title: format!("Your-choice spending was {} lower than usual", money(t.usual - t.spending)),
                body: format!("{last_name}: {} against a usual {}.", money(t.spending), money(t.usual)),
            });
        }

        // Saving this month.
        let saved = self.set_aside(month, today)?;
        if saved.0 > 0 {
            out.push(Win { kind: "saved", title: format!("{} set aside this month", money(saved)), body: "Every bit counts toward what you want.".into() });
        }
        for g in self.goals(today)? {
            if let Some(at) = &g.achieved_at
                && at.starts_with(month)
            {
                out.push(Win { kind: "goal", title: format!("You reached {}!", g.goal.name), body: format!("{} saved.", money(g.saved)) });
            }
        }

        // A new source of income: money this month in an income category
        // that was quiet for the `USUAL_MONTHS` before. Money from the
        // business is the owner's draw, not new income.
        let now = self.income_by_account(month)?;
        let mut before: HashSet<AccountId> = HashSet::new();
        for back in 1..=USUAL_MONTHS {
            before.extend(self.income_by_account(&prev_month(month, back)?)?.into_iter().filter(|(_, c)| c.0 > 0).map(|(a, _)| a));
        }
        let had_history = !before.is_empty();
        for (account, amount) in now {
            let name = self.account(account)?;
            if had_history && amount.0 > 0 && !before.contains(&account) && name.code.as_deref() != Some(tally_core::chart::codes::FROM_THE_BUSINESS) {
                out.push(Win { kind: "new_income", title: format!("New income: {}", name.name), body: format!("{} so far this month.", money(amount)) });
            }
        }
        Ok(out)
    }
}
