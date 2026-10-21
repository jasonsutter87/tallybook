//! People, monthly plans, and how each month is going against its plan.
//!
//! A month is `YYYY-MM`. Its plan is a set of lines, each for a person (who
//! the money is for) or a category (what it was), plus optionally what there
//! is to work with. Plans carry forward: a month without one of its own uses
//! the latest earlier plan, and editing it gives the month its own copy.
//! Only household money is budgeted.

use crate::{BadValue, DueState, Result, Store, StoreError, conversion_err, rule};
use chrono::{Datelike, Months, NaiveDate};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tally_core::budget::{self, Direction, Status, StatusInput};
use tally_core::journal::EntryId;
use tally_core::{AccountId, AccountKind, Cents, Flex, Scope};

pub type PersonId = i64;

/// Household money moved on one account for one person: (account, person,
/// natural-sign amount, number of entries).
pub(crate) type Movement = (AccountId, Option<PersonId>, Cents, i64);
pub type BudgetId = i64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Person {
    pub id: PersonId,
    pub name: String,
    pub color: String,
    pub sort_order: i64,
    pub archived: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NewPerson {
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
}

/// What a budget line is measured against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum BudgetTarget {
    Person(PersonId),
    Account(AccountId),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BudgetLine {
    pub target: BudgetTarget,
    pub amount: Cents,
    #[serde(default)]
    pub direction: Direction,
    #[serde(default = "default_tolerance")]
    pub tolerance_pct: u32,
}

fn default_tolerance() -> u32 {
    10
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BudgetLineView {
    pub id: BudgetId,
    pub target: BudgetTarget,
    pub name: String,
    pub flex: Option<Flex>,
    pub amount: Cents,
    pub direction: Direction,
    pub tolerance_pct: u32,
    /// Spent for a ceiling, put in or earned for a floor.
    pub progress: Cents,
    pub status: Status,
    pub percent_used: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PersonSpend {
    /// `None` is shared household spending nobody was tagged on.
    pub person_id: Option<PersonId>,
    pub name: String,
    pub spending: Cents,
    pub entries: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CategorySpend {
    pub account_id: AccountId,
    pub name: String,
    pub flex: Option<Flex>,
    pub spending: Cents,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MonthBudget {
    pub month: String,
    pub from: NaiveDate,
    pub to: NaiveDate,
    /// The day the statuses are judged at: today in the current month, the
    /// last day for a past one.
    pub day_of_month: u32,
    pub days_in_month: u32,
    pub is_current: bool,
    /// The month this plan was set in, when it was carried forward.
    pub plan_from: Option<String>,
    pub available: Option<Cents>,
    pub income: Cents,
    pub spending: Cents,
    /// Spending against what there is to work with, when that's set.
    pub overall: Option<Status>,
    pub lines: Vec<BudgetLineView>,
    pub planned_for_people: Cents,
    pub planned_for_categories: Cents,
    /// Recurring bills paid this month, and still to come.
    pub bills_paid: Cents,
    pub bills_due: Cents,
    pub people: Vec<PersonSpend>,
    pub categories: Vec<CategorySpend>,
}

/// The first and last day of a `YYYY-MM` month.
pub fn month_range(month: &str) -> Result<(NaiveDate, NaiveDate)> {
    let from = NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d")
        .ok()
        .filter(|_| month.len() == 7)
        .ok_or_else(|| StoreError::Rule(format!("{month:?} isn't a month (use YYYY-MM)")))?;
    let to = from + Months::new(1) - chrono::Days::new(1);
    Ok((from, to))
}

pub fn month_of(date: NaiveDate) -> String {
    date.format("%Y-%m").to_string()
}

fn get<K: std::hash::Hash + Eq>(m: &HashMap<K, Cents>, k: &K) -> Cents {
    m.get(k).copied().unwrap_or_default()
}

const PERSON_COLS: &str = "id, name, color, sort_order, archived";

fn person_from_row(r: &Row) -> rusqlite::Result<Person> {
    Ok(Person { id: r.get(0)?, name: r.get(1)?, color: r.get(2)?, sort_order: r.get(3)?, archived: r.get(4)? })
}

struct PlanRow {
    id: BudgetId,
    target: BudgetTarget,
    amount: Cents,
    direction: Direction,
    tolerance_pct: u32,
}

fn plan_from_row(r: &Row) -> rusqlite::Result<PlanRow> {
    let person: Option<PersonId> = r.get(1)?;
    let account: Option<AccountId> = r.get(2)?;
    let direction: String = r.get(4)?;
    Ok(PlanRow {
        id: r.get(0)?,
        target: match (person, account) {
            (Some(p), _) => BudgetTarget::Person(p),
            (None, Some(a)) => BudgetTarget::Account(a),
            (None, None) => return Err(conversion_err(1, BadValue("budget line without a target".into()))),
        },
        amount: Cents(r.get(3)?),
        direction: Direction::parse(&direction).ok_or_else(|| conversion_err(4, BadValue(direction)))?,
        tolerance_pct: r.get(5)?,
    })
}

impl Store {
    // ------------------------------------------------------------ people

    pub fn people(&self) -> Result<Vec<Person>> {
        let mut stmt = self.conn.prepare(&format!("SELECT {PERSON_COLS} FROM people ORDER BY archived, sort_order, id"))?;
        Ok(stmt.query_map([], person_from_row)?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn person(&self, id: PersonId) -> Result<Person> {
        self.conn
            .query_row(&format!("SELECT {PERSON_COLS} FROM people WHERE id = ?1"), [id], person_from_row)
            .optional()?
            .ok_or(StoreError::NotFound("person", id))
    }

    pub fn create_person(&self, p: &NewPerson) -> Result<PersonId> {
        let name = p.name.trim();
        if name.is_empty() {
            return rule("give them a name");
        }
        let order: i64 = self.conn.query_row("SELECT IFNULL(MAX(sort_order), 0) + 1 FROM people", [], |r| r.get(0))?;
        self.conn.execute(
            "INSERT INTO people(name, color, sort_order) VALUES (?1, ?2, ?3)",
            params![name, p.color.as_deref().unwrap_or("slate"), order],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_person(&self, id: PersonId, p: &NewPerson) -> Result<()> {
        let name = p.name.trim();
        if name.is_empty() {
            return rule("give them a name");
        }
        let current = self.person(id)?;
        self.conn.execute(
            "UPDATE people SET name = ?1, color = ?2 WHERE id = ?3",
            params![name, p.color.as_deref().unwrap_or(&current.color), id],
        )?;
        Ok(())
    }

    /// Archived people keep their history but drop out of pickers and new plans.
    pub fn set_person_archived(&self, id: PersonId, archived: bool) -> Result<()> {
        self.person(id)?;
        self.conn.execute("UPDATE people SET archived = ?1 WHERE id = ?2", params![archived, id])?;
        Ok(())
    }

    /// Who an entry's money was for. Metadata, so it may change after posting.
    pub fn set_entry_person(&self, entry_id: EntryId, person_id: Option<PersonId>) -> Result<()> {
        if let Some(p) = person_id {
            self.person(p)?;
        }
        if self.conn.execute("UPDATE journal_entries SET person_id = ?1 WHERE id = ?2", params![person_id, entry_id])? == 0 {
            return Err(StoreError::NotFound("entry", entry_id));
        }
        Ok(())
    }

    // ------------------------------------------------------------ plans

    /// The month whose plan applies to `month`: itself, or the latest earlier one.
    fn plan_month(&self, month: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT month FROM budget_months WHERE month <= ?1 ORDER BY month DESC LIMIT 1", [month], |r| r.get(0))
            .optional()?)
    }

    fn plan_rows(&self, plan_month: &str) -> Result<Vec<PlanRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, person_id, account_id, amount, direction, tolerance_pct FROM budgets WHERE month = ?1 ORDER BY id",
        )?;
        Ok(stmt.query_map([plan_month], plan_from_row)?.collect::<rusqlite::Result<_>>()?)
    }

    /// Gives `month` its own copy of the plan it inherits, so it can be
    /// edited without changing the months before it.
    fn own_plan(&mut self, month: &str) -> Result<()> {
        month_range(month)?;
        let from = self.plan_month(month)?;
        if from.as_deref() == Some(month) {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        match &from {
            Some(prev) => {
                tx.execute("INSERT INTO budget_months(month, available) SELECT ?1, available FROM budget_months WHERE month = ?2", params![month, prev])?;
                tx.execute(
                    "INSERT INTO budgets(month, person_id, account_id, amount, direction, tolerance_pct)
                     SELECT ?1, b.person_id, b.account_id, b.amount, b.direction, b.tolerance_pct
                     FROM budgets b
                     LEFT JOIN people p ON p.id = b.person_id
                     LEFT JOIN accounts a ON a.id = b.account_id
                     WHERE b.month = ?2 AND IFNULL(p.archived, 0) = 0 AND IFNULL(a.archived, 0) = 0
                     ORDER BY b.id",
                    params![month, prev],
                )?;
            }
            None => {
                tx.execute("INSERT INTO budget_months(month) VALUES (?1)", [month])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Adds or changes one line of a month's plan.
    pub fn set_budget_line(&mut self, month: &str, line: &BudgetLine) -> Result<BudgetId> {
        if line.amount.0 < 0 {
            return rule("a budget can't be negative");
        }
        if line.tolerance_pct > 100 {
            return rule("tolerance is a percent from 0 to 100");
        }
        let (person, account) = match line.target {
            BudgetTarget::Person(p) => {
                if self.person(p)?.archived {
                    return rule("that person is archived");
                }
                if line.direction == Direction::Floor {
                    return rule("a person's budget is a limit on spending");
                }
                (Some(p), None)
            }
            BudgetTarget::Account(a) => {
                let acct = self.account(a)?;
                if acct.scope != Scope::Household {
                    return rule("only household categories are budgeted");
                }
                if !matches!(acct.kind(), AccountKind::Expense | AccountKind::Income | AccountKind::Asset) {
                    return rule(format!("{} can't be budgeted; choose spending, income, or a savings account", acct.name));
                }
                (None, Some(a))
            }
        };
        self.own_plan(month)?;
        let existing: Option<BudgetId> = self
            .conn
            .query_row(
                "SELECT id FROM budgets WHERE month = ?1 AND person_id IS ?2 AND account_id IS ?3",
                params![month, person, account],
                |r| r.get(0),
            )
            .optional()?;
        match existing {
            Some(id) => {
                self.conn.execute(
                    "UPDATE budgets SET amount = ?1, direction = ?2, tolerance_pct = ?3 WHERE id = ?4",
                    params![line.amount.0, line.direction.as_str(), line.tolerance_pct, id],
                )?;
                Ok(id)
            }
            None => {
                self.conn.execute(
                    "INSERT INTO budgets(month, person_id, account_id, amount, direction, tolerance_pct) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![month, person, account, line.amount.0, line.direction.as_str(), line.tolerance_pct],
                )?;
                Ok(self.conn.last_insert_rowid())
            }
        }
    }

    /// Takes a target off a month's plan (and, by carrying forward, later months').
    pub fn remove_budget_line(&mut self, month: &str, target: BudgetTarget) -> Result<()> {
        self.own_plan(month)?;
        let (person, account) = match target {
            BudgetTarget::Person(p) => (Some(p), None),
            BudgetTarget::Account(a) => (None, Some(a)),
        };
        self.conn.execute(
            "DELETE FROM budgets WHERE month = ?1 AND person_id IS ?2 AND account_id IS ?3",
            params![month, person, account],
        )?;
        Ok(())
    }

    /// What there is to work with this month; `None` clears it.
    pub fn set_available(&mut self, month: &str, available: Option<Cents>) -> Result<()> {
        if available.is_some_and(|a| a.0 < 0) {
            return rule("what you have to work with can't be negative");
        }
        self.own_plan(month)?;
        self.conn.execute("UPDATE budget_months SET available = ?1 WHERE month = ?2", params![available.map(|a| a.0), month])?;
        Ok(())
    }

    // ------------------------------------------------------------ the month

    /// Household money moved in `[from, to]`, natural sign, per account and
    /// person. Voided entries and their reversals are left out entirely: a
    /// void means it never happened, whichever month the void is dated in.
    pub(crate) fn household_movement(&self, from: NaiveDate, to: NaiveDate) -> Result<Vec<Movement>> {
        let accounts: HashMap<AccountId, AccountKind> =
            self.accounts_in(Some(Scope::Household))?.into_iter().map(|a| (a.id, a.kind())).collect();
        let mut stmt = self.conn.prepare_cached(
            "SELECT l.account_id, e.person_id, SUM(l.amount), COUNT(DISTINCT e.id)
             FROM journal_lines l
             JOIN journal_entries e ON e.id = l.entry_id
             JOIN accounts a ON a.id = l.account_id
             WHERE a.scope = 'household' AND e.date BETWEEN ?1 AND ?2
               AND e.voided_by_id IS NULL AND e.reverses_id IS NULL
             GROUP BY l.account_id, e.person_id",
        )?;
        let rows = stmt
            .query_map(params![from, to], |r| Ok((r.get::<_, AccountId>(0)?, r.get(1)?, Cents(r.get(2)?), r.get(3)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows.into_iter().filter_map(|(a, p, raw, n)| accounts.get(&a).map(|k| (a, p, k.natural(raw), n))).collect())
    }

    /// How `month` is going against its plan, judged as of `today`.
    pub fn month_budget(&self, month: &str, today: NaiveDate) -> Result<MonthBudget> {
        let (from, to) = month_range(month)?;
        let days_in_month = to.day();
        let is_current = (from..=to).contains(&today);
        let day_of_month = if is_current { today.day() } else if today > to { days_in_month } else { 1 };

        let accounts: HashMap<AccountId, tally_core::Account> =
            self.accounts_in(Some(Scope::Household))?.into_iter().map(|a| (a.id, a)).collect();
        let people = self.people()?;
        let movement = self.household_movement(from, to)?;

        let kind_of = |id: AccountId| accounts.get(&id).map(|a| a.kind());
        let mut income = Cents::ZERO;
        let mut spending = Cents::ZERO;
        let mut by_account: HashMap<AccountId, Cents> = HashMap::new();
        let mut by_person: HashMap<Option<PersonId>, (Cents, i64)> = HashMap::new();
        // Spending in fixed categories (rent, a car payment) lands once a
        // month on its day, so it's counted as-is rather than projected.
        let is_fixed = |id: AccountId| accounts.get(&id).is_some_and(|a| a.flex == Some(Flex::Fixed));
        let mut fixed = Cents::ZERO;
        let mut fixed_by_person: HashMap<Option<PersonId>, Cents> = HashMap::new();
        for (account, person, amount, n) in &movement {
            *by_account.entry(*account).or_default() += *amount;
            match kind_of(*account) {
                Some(AccountKind::Income) => income += *amount,
                Some(AccountKind::Expense) => {
                    spending += *amount;
                    let p = by_person.entry(*person).or_default();
                    p.0 += *amount;
                    p.1 += n;
                    if is_fixed(*account) {
                        fixed += *amount;
                        *fixed_by_person.entry(*person).or_default() += *amount;
                    }
                }
                _ => {}
            }
        }

        let plan_month = self.plan_month(month)?;
        let available: Option<Cents> = match &plan_month {
            Some(m) => self
                .conn
                .query_row("SELECT available FROM budget_months WHERE month = ?1", [m], |r| r.get::<_, Option<i64>>(0))?
                .map(Cents),
            None => None,
        };
        let rows = match &plan_month {
            Some(m) => self.plan_rows(m)?,
            None => vec![],
        };

        // Recurring bills: paid ones count as-is (like fixed categories), and
        // ones still to come are added to the projection in full.
        let mut bill_paid: HashMap<AccountId, Cents> = HashMap::new();
        let mut bill_due: HashMap<AccountId, Cents> = HashMap::new();
        let mut bill_paid_by_person: HashMap<PersonId, Cents> = HashMap::new();
        let mut bill_due_by_person: HashMap<PersonId, Cents> = HashMap::new();
        for d in self.due_dates(from, to, today)? {
            match (d.state, &d.payment) {
                (DueState::Paid, Some(p)) if (from..=to).contains(&p.date) && !is_fixed(d.account_id) => {
                    *bill_paid.entry(d.account_id).or_default() += p.amount;
                    if let Some(person) = d.person_id {
                        *bill_paid_by_person.entry(person).or_default() += p.amount;
                    }
                }
                (DueState::DueToday | DueState::Upcoming | DueState::Processing, _) if today <= to => {
                    *bill_due.entry(d.account_id).or_default() += d.amount;
                    if let Some(person) = d.person_id {
                        *bill_due_by_person.entry(person).or_default() += d.amount;
                    }
                }
                _ => {}
            }
        }
        let bills_paid: Cents = bill_paid.values().copied().sum();
        let bills_due: Cents = bill_due.values().copied().sum();

        let judge = |allocated: Cents, progress: Cents, direction: Direction, tolerance_pct: u32, scheduled: Cents, due: Cents| {
            let input = StatusInput::new(allocated, progress, direction, tolerance_pct, day_of_month, days_in_month);
            budget::status(&StatusInput { scheduled_paid: scheduled.clamp(Cents::ZERO, progress.max(Cents::ZERO)), scheduled_due: due, ..input })
        };
        let mut lines = Vec::new();
        let (mut planned_for_people, mut planned_for_categories) = (Cents::ZERO, Cents::ZERO);
        for r in rows {
            let (name, flex, progress, scheduled, due) = match r.target {
                BudgetTarget::Person(p) => {
                    let Some(person) = people.iter().find(|x| x.id == p && !x.archived) else { continue };
                    planned_for_people += r.amount;
                    let scheduled = get(&fixed_by_person, &Some(p)) + get(&bill_paid_by_person, &p);
                    (person.name.clone(), None, by_person.get(&Some(p)).map(|x| x.0).unwrap_or_default(), scheduled, get(&bill_due_by_person, &p))
                }
                BudgetTarget::Account(a) => {
                    let Some(acct) = accounts.get(&a).filter(|x| !x.archived) else { continue };
                    if acct.kind() == AccountKind::Expense {
                        planned_for_categories += r.amount;
                    }
                    let progress = by_account.get(&a).copied().unwrap_or_default();
                    let scheduled = if is_fixed(a) { progress } else { get(&bill_paid, &a) };
                    (acct.name.clone(), acct.flex, progress, scheduled, get(&bill_due, &a))
                }
            };
            let status = judge(r.amount, progress.max(Cents::ZERO), r.direction, r.tolerance_pct, scheduled, due);
            let percent_used = (r.direction == Direction::Ceiling).then(|| budget::percent_used(r.amount, progress)).flatten();
            lines.push(BudgetLineView {
                id: r.id,
                target: r.target,
                name,
                flex,
                amount: r.amount,
                direction: r.direction,
                tolerance_pct: r.tolerance_pct,
                progress,
                status,
                percent_used,
            });
        }

        let mut people_spend: Vec<PersonSpend> = people
            .iter()
            .filter(|p| !p.archived || by_person.contains_key(&Some(p.id)))
            .map(|p| {
                let (spent, n) = by_person.get(&Some(p.id)).copied().unwrap_or_default();
                PersonSpend { person_id: Some(p.id), name: p.name.clone(), spending: spent, entries: n }
            })
            .collect();
        let (shared, n) = by_person.get(&None).copied().unwrap_or_default();
        people_spend.push(PersonSpend { person_id: None, name: "Shared".into(), spending: shared, entries: n });

        let mut categories: Vec<CategorySpend> = by_account
            .iter()
            .filter_map(|(id, amount)| {
                let a = accounts.get(id)?;
                (a.kind() == AccountKind::Expense && !amount.is_zero()).then(|| CategorySpend { account_id: *id, name: a.name.clone(), flex: a.flex, spending: *amount })
            })
            .collect();
        categories.sort_by(|a, b| b.spending.cmp(&a.spending).then_with(|| a.name.cmp(&b.name)));

        Ok(MonthBudget {
            month: month.to_string(),
            from,
            to,
            day_of_month,
            days_in_month,
            is_current,
            plan_from: plan_month.filter(|m| m != month),
            available,
            income,
            spending,
            overall: available.map(|a| judge(a, spending.max(Cents::ZERO), Direction::Ceiling, 10, fixed + bills_paid, bills_due)),
            lines,
            planned_for_people,
            planned_for_categories,
            bills_paid,
            bills_due,
            people: people_spend,
            categories,
        })
    }
}
