//! Household bills that come around again, and whether each due date has
//! been paid.
//!
//! Nothing here is posted to the books. A bill is a reminder with a
//! category; a due date counts as paid when a household payment into that
//! category lands near it, for about the right amount. The user can also
//! mark a date paid or skipped by hand, which always wins.

use crate::{BadValue, PersonId, Result, Store, StoreError, conversion_err, rule};
use chrono::{Datelike, Days, Months, NaiveDate};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tally_core::bank::normalize_description;
use tally_core::journal::EntryId;
use tally_core::schedule::{self, Cadence};
use tally_core::{AccountId, AccountKind, AccountSubtype, Cents, Scope};

pub type RecurringBillId = i64;

/// How close to its due date a payment can land and still count.
const MATCH_DAYS: i64 = 10;
/// Auto-pay bills get a few days' grace before they're called overdue.
const AUTO_PAY_GRACE_DAYS: i64 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewRecurringBill {
    pub name: String,
    /// Zero when it varies.
    #[serde(default)]
    pub amount: Cents,
    pub account_id: AccountId,
    #[serde(default)]
    pub paid_from_id: Option<AccountId>,
    #[serde(default)]
    pub person_id: Option<PersonId>,
    pub cadence: Cadence,
    pub start_date: NaiveDate,
    #[serde(default)]
    pub auto_pay: bool,
    #[serde(default)]
    pub match_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecurringBill {
    pub id: RecurringBillId,
    #[serde(flatten)]
    pub bill: NewRecurringBill,
    pub active: bool,
    pub next_due: Option<NaiveDate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DueState {
    Paid,
    Skipped,
    /// Past due with no payment seen.
    Overdue,
    /// Auto-pay, just past due: the payment is probably on its way.
    Processing,
    DueToday,
    Upcoming,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Payment {
    pub entry_id: EntryId,
    pub date: NaiveDate,
    pub amount: Cents,
    /// Marked by hand rather than found.
    pub manual: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DueDate {
    pub bill_id: RecurringBillId,
    pub name: String,
    pub amount: Cents,
    pub account_id: AccountId,
    pub person_id: Option<PersonId>,
    pub auto_pay: bool,
    pub due_date: NaiveDate,
    pub state: DueState,
    pub payment: Option<Payment>,
}

/// A repeat payment spotted in the books that isn't a bill yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BillSuggestion {
    pub name: String,
    pub account_id: AccountId,
    pub amount: Cents,
    pub day: u32,
    pub match_text: String,
    pub months_seen: usize,
    pub paid_from_id: Option<AccountId>,
}

const BILL_COLS: &str = "id, name, amount, account_id, paid_from_id, person_id, cadence, start_date, auto_pay, match_text, active";

fn bill_from_row(r: &Row) -> rusqlite::Result<(RecurringBillId, NewRecurringBill, bool)> {
    let cadence: String = r.get(6)?;
    Ok((
        r.get(0)?,
        NewRecurringBill {
            name: r.get(1)?,
            amount: Cents(r.get(2)?),
            account_id: r.get(3)?,
            paid_from_id: r.get(4)?,
            person_id: r.get(5)?,
            cadence: serde_json::from_str(&cadence).map_err(|_| conversion_err(6, BadValue(cadence)))?,
            start_date: r.get(7)?,
            auto_pay: r.get(8)?,
            match_text: r.get(9)?,
        },
        r.get(10)?,
    ))
}

/// A household payment into a category: something a due date could match.
struct Candidate {
    entry_id: EntryId,
    date: NaiveDate,
    account_id: AccountId,
    amount: Cents,
    /// Memo and bank description, normalized, for `match_text`.
    text: String,
}

fn amount_close(bill: Cents, paid: Cents) -> bool {
    if bill.is_zero() {
        return paid.0 > 0;
    }
    // Within 10%, or $5 for small bills.
    let slack = (bill.0 / 10).max(500);
    (paid.0 - bill.0).abs() <= slack
}

impl Store {
    pub fn recurring_bills(&self, today: NaiveDate) -> Result<Vec<RecurringBill>> {
        let mut stmt = self.conn.prepare(&format!("SELECT {BILL_COLS} FROM recurring_bills ORDER BY active DESC, name"))?;
        let rows = stmt.query_map([], bill_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows
            .into_iter()
            .map(|(id, bill, active)| {
                let next_due = active.then(|| schedule::next_due(bill.cadence, bill.start_date, today)).flatten();
                RecurringBill { id, bill, active, next_due }
            })
            .collect())
    }

    fn recurring_bill(&self, id: RecurringBillId) -> Result<(NewRecurringBill, bool)> {
        self.conn
            .query_row(&format!("SELECT {BILL_COLS} FROM recurring_bills WHERE id = ?1"), [id], bill_from_row)
            .optional()?
            .map(|(_, b, a)| (b, a))
            .ok_or(StoreError::NotFound("recurring bill", id))
    }

    fn validate_bill(&self, b: &NewRecurringBill) -> Result<()> {
        if b.name.trim().is_empty() {
            return rule("give the bill a name");
        }
        if b.amount.0 < 0 {
            return rule("a bill's amount can't be negative");
        }
        b.cadence.validate().map_err(|m| StoreError::Rule(m.into()))?;
        let category = self.account(b.account_id)?;
        if category.scope != Scope::Household || category.kind() != AccountKind::Expense {
            return rule("choose a household spending category for the bill");
        }
        if let Some(from) = b.paid_from_id {
            let a = self.account(from)?;
            if a.scope != Scope::Household || !matches!(a.subtype, AccountSubtype::Bank | AccountSubtype::CreditCard) {
                return rule("a bill is paid from a household bank account or card");
            }
        }
        if let Some(p) = b.person_id {
            self.person(p)?;
        }
        Ok(())
    }

    pub fn create_recurring_bill(&self, b: &NewRecurringBill) -> Result<RecurringBillId> {
        self.validate_bill(b)?;
        self.conn.execute(
            "INSERT INTO recurring_bills(name, amount, account_id, paid_from_id, person_id, cadence, start_date, auto_pay, match_text)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                b.name.trim(),
                b.amount.0,
                b.account_id,
                b.paid_from_id,
                b.person_id,
                serde_json::to_string(&b.cadence).expect("cadence serializes"),
                b.start_date,
                b.auto_pay,
                b.match_text.as_deref().map(str::trim).filter(|s| !s.is_empty())
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_recurring_bill(&self, id: RecurringBillId, b: &NewRecurringBill) -> Result<()> {
        self.validate_bill(b)?;
        self.recurring_bill(id)?;
        self.conn.execute(
            "UPDATE recurring_bills SET name = ?1, amount = ?2, account_id = ?3, paid_from_id = ?4, person_id = ?5,
                    cadence = ?6, start_date = ?7, auto_pay = ?8, match_text = ?9 WHERE id = ?10",
            params![
                b.name.trim(),
                b.amount.0,
                b.account_id,
                b.paid_from_id,
                b.person_id,
                serde_json::to_string(&b.cadence).expect("cadence serializes"),
                b.start_date,
                b.auto_pay,
                b.match_text.as_deref().map(str::trim).filter(|s| !s.is_empty()),
                id
            ],
        )?;
        Ok(())
    }

    /// Stopped bills keep their history but stop coming due.
    pub fn set_recurring_bill_active(&self, id: RecurringBillId, active: bool) -> Result<()> {
        self.recurring_bill(id)?;
        self.conn.execute("UPDATE recurring_bills SET active = ?1 WHERE id = ?2", params![active, id])?;
        Ok(())
    }

    /// Marks one due date paid (optionally by a specific entry) or skipped;
    /// `None` goes back to automatic matching.
    pub fn mark_due_date(&self, id: RecurringBillId, due: NaiveDate, status: Option<DueState>, entry_id: Option<EntryId>) -> Result<()> {
        let (bill, _) = self.recurring_bill(id)?;
        if !schedule::occurrences(bill.cadence, bill.start_date, due, due).contains(&due) {
            return rule(format!("{} isn't due on {due}", bill.name));
        }
        if let Some(e) = entry_id {
            self.entry(e)?;
        }
        match status {
            None => {
                self.conn.execute("DELETE FROM recurring_bill_marks WHERE bill_id = ?1 AND due_date = ?2", params![id, due])?;
            }
            Some(s @ (DueState::Paid | DueState::Skipped)) => {
                let s = if s == DueState::Paid { "paid" } else { "skipped" };
                self.conn.execute(
                    "INSERT INTO recurring_bill_marks(bill_id, due_date, status, entry_id) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(bill_id, due_date) DO UPDATE SET status = excluded.status, entry_id = excluded.entry_id",
                    params![id, due, s, entry_id.filter(|_| s == "paid")],
                )?;
            }
            Some(_) => return rule("a due date can only be marked paid or skipped"),
        }
        Ok(())
    }

    fn candidates(&self, from: NaiveDate, to: NaiveDate) -> Result<Vec<Candidate>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT e.id, e.date, l.account_id, SUM(l.amount), e.memo || ' ' || IFNULL(bt.description, '') || ' ' || IFNULL(bt.merchant, '')
             FROM journal_lines l
             JOIN journal_entries e ON e.id = l.entry_id
             JOIN accounts a ON a.id = l.account_id
             LEFT JOIN bank_transactions bt ON bt.entry_id = e.id
             WHERE a.scope = 'household' AND a.subtype IN ('expense', 'other_expense')
               AND e.date BETWEEN ?1 AND ?2 AND e.voided_by_id IS NULL AND e.reverses_id IS NULL
             GROUP BY e.id, l.account_id
             HAVING SUM(l.amount) > 0
             ORDER BY e.date, e.id",
        )?;
        let rows = stmt
            .query_map(params![from, to], |r| {
                let text: String = r.get(4)?;
                Ok(Candidate { entry_id: r.get(0)?, date: r.get(1)?, account_id: r.get(2)?, amount: Cents(r.get(3)?), text: normalize_description(&text) })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Every due date of every active bill in `[from, to]`, with whether
    /// it's been paid, judged as of `today`.
    pub fn due_dates(&self, from: NaiveDate, to: NaiveDate, today: NaiveDate) -> Result<Vec<DueDate>> {
        let bills = self.recurring_bills(today)?;
        // Match from a little earlier, so a payment already claimed by last
        // month's due date isn't counted again for this one.
        let lookback = from - Days::new(45);
        let candidates = self.candidates(lookback - Days::new(MATCH_DAYS as u64), to + Days::new(MATCH_DAYS as u64))?;
        let marks: HashMap<(RecurringBillId, NaiveDate), (String, Option<EntryId>)> = {
            let mut stmt = self.conn.prepare("SELECT bill_id, due_date, status, entry_id FROM recurring_bill_marks")?;
            stmt.query_map([], |r| Ok(((r.get(0)?, r.get(1)?), (r.get(2)?, r.get(3)?))))?.collect::<rusqlite::Result<_>>()?
        };
        let mut claimed: HashSet<(EntryId, AccountId)> =
            marks.values().filter_map(|(_, e)| *e).flat_map(|e| candidates.iter().filter(move |c| c.entry_id == e).map(|c| (c.entry_id, c.account_id))).collect();

        let mut all: Vec<(&RecurringBill, NaiveDate)> = bills
            .iter()
            .filter(|b| b.active)
            .flat_map(|b| schedule::occurrences(b.bill.cadence, b.bill.start_date, lookback, to).into_iter().map(move |d| (b, d)))
            .collect();
        all.sort_by_key(|(b, d)| (*d, b.id));

        let mut out = Vec::new();
        for (b, due) in all {
            let mark = marks.get(&(b.id, due));
            let payment = match mark {
                Some((s, entry)) if s == "paid" => Some(match entry.and_then(|e| candidates.iter().find(|c| c.entry_id == e)) {
                    Some(c) => Payment { entry_id: c.entry_id, date: c.date, amount: c.amount, manual: true },
                    None => Payment { entry_id: entry.unwrap_or_default(), date: due, amount: b.bill.amount, manual: true },
                }),
                Some(_) => None,
                None => {
                    let needle = b.bill.match_text.as_deref().map(normalize_description).filter(|t| !t.is_empty());
                    let found = candidates
                        .iter()
                        .filter(|c| {
                            c.account_id == b.bill.account_id
                                && (c.date - due).num_days().abs() <= MATCH_DAYS
                                && amount_close(b.bill.amount, c.amount)
                                && needle.as_ref().is_none_or(|n| c.text.contains(n.as_str()))
                                && !claimed.contains(&(c.entry_id, c.account_id))
                        })
                        .min_by_key(|c| (c.date - due).num_days().abs());
                    found.map(|c| {
                        claimed.insert((c.entry_id, c.account_id));
                        Payment { entry_id: c.entry_id, date: c.date, amount: c.amount, manual: false }
                    })
                }
            };
            if due < from {
                continue;
            }
            let state = match (mark.map(|m| m.0.as_str()), &payment) {
                (Some("skipped"), _) => DueState::Skipped,
                (_, Some(_)) => DueState::Paid,
                _ if due == today => DueState::DueToday,
                _ if due > today => DueState::Upcoming,
                _ if b.bill.auto_pay && (today - due).num_days() <= AUTO_PAY_GRACE_DAYS => DueState::Processing,
                _ => DueState::Overdue,
            };
            out.push(DueDate {
                bill_id: b.id,
                name: b.bill.name.clone(),
                amount: b.bill.amount,
                account_id: b.bill.account_id,
                person_id: b.bill.person_id,
                auto_pay: b.bill.auto_pay,
                due_date: due,
                state,
                payment,
            });
        }
        Ok(out)
    }

    /// Payments that repeat month after month but aren't bills yet: the same
    /// payee, into the same category, for about the same amount, around the
    /// same day, in at least three of the last four months.
    pub fn bill_suggestions(&self, today: NaiveDate) -> Result<Vec<BillSuggestion>> {
        let this_month = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).expect("valid");
        let from = this_month - Months::new(4);
        let mut stmt = self.conn.prepare(
            "SELECT e.date, l.account_id, l.amount, COALESCE(bt.normalized, lower(e.memo)), COALESCE(bt.merchant, e.memo), bt.ledger_account_id
             FROM journal_lines l
             JOIN journal_entries e ON e.id = l.entry_id
             JOIN accounts a ON a.id = l.account_id
             LEFT JOIN bank_transactions bt ON bt.entry_id = e.id
             WHERE a.scope = 'household' AND a.subtype IN ('expense', 'other_expense') AND l.amount > 0
               AND e.date >= ?1 AND e.date < ?2 AND e.voided_by_id IS NULL AND e.reverses_id IS NULL",
        )?;
        type Seen = (NaiveDate, Cents, String, Option<AccountId>);
        let mut groups: HashMap<(String, AccountId), Vec<Seen>> = HashMap::new();
        for row in stmt.query_map(params![from, this_month], |r| {
            Ok((r.get::<_, NaiveDate>(0)?, r.get::<_, AccountId>(1)?, Cents(r.get(2)?), r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get(5)?))
        })? {
            let (date, account, amount, key, name, bank) = row?;
            let key = normalize_description(&key);
            if key.len() >= 3 {
                groups.entry((key, account)).or_default().push((date, amount, name, bank));
            }
        }
        let existing = self.recurring_bills(today)?;
        let mut out = Vec::new();
        for ((key, account_id), mut seen) in groups {
            let months: HashSet<(i32, u32)> = seen.iter().map(|s| (s.0.year(), s.0.month())).collect();
            if months.len() < 3 {
                continue;
            }
            seen.sort_by_key(|s| s.1);
            let median = seen[seen.len() / 2].1;
            if !seen.iter().all(|s| amount_close(median, s.1)) {
                continue;
            }
            let mut days: Vec<u32> = seen.iter().map(|s| s.0.day()).collect();
            days.sort_unstable();
            if days[days.len() - 1] - days[0] > 7 {
                continue;
            }
            let covered = existing.iter().any(|b| {
                b.bill.account_id == account_id
                    && b.bill.match_text.as_deref().map(normalize_description).is_none_or(|t| key.contains(&t) || t.contains(&key))
            });
            if covered {
                continue;
            }
            out.push(BillSuggestion {
                name: seen[0].2.trim().to_string(),
                account_id,
                amount: median,
                day: days[days.len() / 2],
                match_text: key,
                months_seen: months.len(),
                paid_from_id: seen[0].3,
            });
        }
        out.sort_by(|a, b| a.day.cmp(&b.day).then_with(|| a.name.cmp(&b.name)));
        Ok(out)
    }
}
