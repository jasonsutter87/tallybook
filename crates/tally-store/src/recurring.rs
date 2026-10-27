//! Household bills that come around again, and whether each due date has
//! been paid.
//!
//! Nothing here is posted to the books. A bill is a reminder with a
//! category; a due date counts as paid when a household payment into that
//! category lands near it, for about the right amount. The user can also
//! mark a date paid or skipped by hand, which always wins.
//!
//! It also spots bills nobody has set up yet (`bill_suggestions`): the same
//! payee paid on a schedule, in the books or in a fresh bank import. Bank
//! text is untrusted: whoever sends or requests a payment writes its
//! description. So that text is capped in length, stripped of invisible and
//! direction-flipping characters, shown with look-alike letters masked, and
//! never lets one payee's payments tick off, or hide, another's.

use crate::{insert_with_id, BadValue, PersonId, Result, Store, StoreError, Suggestion, conversion_err, rule};
use chrono::{Days, NaiveDate};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tally_core::bank::normalize_description;
use tally_core::journal::EntryId;
use tally_core::schedule::{self, Cadence};
use tally_core::{Account, AccountId, AccountKind, AccountSubtype, Cents, Scope};

pub type RecurringBillId = i64;

/// How close to its due date a payment can land and still count.
const MATCH_DAYS: i64 = 10;
/// Auto-pay bills get a few days' grace before they're called overdue.
const AUTO_PAY_GRACE_DAYS: i64 = 3;
/// Due dates are matched from this many days before the asked-for range, so
/// a payment already claimed by last month's due date isn't counted again.
const CLAIMED_LOOKBACK_DAYS: u64 = 45;
/// A steady bill's payment may be off by a tenth of its amount...
const STEADY_SLACK_DIVISOR: i64 = 10;
/// ...or by this much, for small bills where a tenth is pennies ($5.00).
const STEADY_MIN_SLACK: Cents = Cents(500);

/// `recurring_bill_marks.status` for a due date the user marked paid or skipped.
const MARK_PAID: &str = "paid";
const MARK_SKIPPED: &str = "skipped";

/// A household bill as the user sets it up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewRecurringBill {
    pub name: String,
    /// What it usually comes to; zero when that isn't known.
    #[serde(default)]
    pub amount: Cents,
    /// The amount moves around (power, water): `amount` is the typical one.
    #[serde(default)]
    pub varies: bool,
    /// The household spending category it's filed under; only payments into
    /// it can tick a due date off.
    pub account_id: AccountId,
    /// The household bank account or card it's usually paid from.
    #[serde(default)]
    pub paid_from_id: Option<AccountId>,
    /// Who it's for, when it's one person's.
    #[serde(default)]
    pub person_id: Option<PersonId>,
    /// Stored as JSON in `recurring_bills.cadence` (see `Cadence`).
    pub cadence: Cadence,
    /// The first due date; the cadence counts from here.
    pub start_date: NaiveDate,
    /// Paid automatically: just past due it's `Processing`, not `Overdue`.
    #[serde(default)]
    pub auto_pay: bool,
    /// Bank text a payment must contain to count, e.g. `netflix.com`, as
    /// `normalize_description` writes it, plus any long ids it must carry
    /// (a suggestion's payee key). `None` lets any payment into the category count.
    #[serde(default)]
    pub match_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecurringBill {
    pub id: RecurringBillId,
    #[serde(flatten)]
    pub bill: NewRecurringBill,
    /// Stopped bills keep their history but stop coming due.
    pub active: bool,
    /// The next due date on or after today; `None` once stopped.
    pub next_due: Option<NaiveDate>,
}

/// Where one due date stands.
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

/// The payment that settled a due date.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Payment {
    pub entry_id: EntryId,
    pub date: NaiveDate,
    pub amount: Cents,
    /// Marked by hand rather than found.
    pub manual: bool,
}

/// One time a bill comes due, with whether it's been paid.
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

/// A repeat payment spotted in the books, or in a fresh bank import, that
/// isn't a bill yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BillSuggestion {
    pub name: String,
    /// The category it's usually filed under; `None` when it's only been
    /// seen in Review, uncategorized.
    pub account_id: Option<AccountId>,
    /// The typical amount.
    pub amount: Cents,
    /// The amount moves around (power, water).
    pub varies: bool,
    pub cadence: Cadence,
    pub start_date: NaiveDate,
    pub last_seen: NaiveDate,
    pub times_seen: usize,
    /// How many of those are still waiting in Review.
    pub in_review: usize,
    /// The payee key the payments were grouped on (`payee_key`), ready to be
    /// the bill's `match_text` or to dismiss with "Not a bill".
    pub match_text: String,
    pub paid_from_id: Option<AccountId>,
    pub person_id: Option<PersonId>,
}

/// Settings key for payees the user waved off with "Not a bill". The value is
/// a JSON array of `match_text`s as `normalize_description` writes them, kept
/// sorted, e.g. `["netflix.com","venmo *bob"]`, at most `MAX_DISMISSED` of
/// them, each cut to `MAX_PAYEE_CHARS`. A value that won't parse
/// reads as empty: suggestions come back rather than the page failing.
const DISMISSED_KEY: &str = "bill_suggestions.dismissed";

/// How far back to look for repeat payments: two years and ten weeks, so a
/// yearly bill paid a little late is still seen twice.
const LOOKBACK_DAYS: u64 = 800;

/// Shorter normalized payees ("atm", "") say too little to group on.
const MIN_PAYEE_LEN: usize = 3;

/// Bank text is whatever the payee or a Venmo memo says, so the payee key
/// kept from it, and the name shown for it, are capped (in characters).
/// Real payees fit easily.
const MAX_PAYEE_CHARS: usize = 120;
const MAX_NAME_CHARS: usize = 80;

/// "Not a bill" waved off this many payees at most, so the settings row
/// can't grow without end.
const MAX_DISMISSED: usize = 1000;

/// No household bill comes to more than $10 million. Bank text can carry any
/// amount, and bills are summed into budgets, which mustn't overflow.
pub const MAX_BILL_AMOUNT: Cents = Cents(1_000_000_000); // $10,000,000.00

/// A bill that varies counts a payment from half to double its typical
/// amount: a cold month's power bill, not a $5,000 charge that names the payee.
const VARIES_MIN_PERCENT: i128 = 50;
const VARIES_MAX_PERCENT: i128 = 200;

/// Digit runs this long name who's charging: an ACH originator's id, a phone
/// number. Shorter ones are store numbers and dates ("SAFEWAY #1234",
/// "PGE WEB ONLINE 0518"), dropped so those still group.
const ID_DIGITS: usize = 7;
/// At most this many ids are read from one payment, and kept in a payee key,
/// each cut to `MAX_ID_DIGITS`.
const IDS_READ: usize = 8;
const MAX_IDS: usize = 2;
const MAX_ID_DIGITS: usize = 20;

/// Review rows read for suggestions, newest first. More than a household
/// imports in months; it keeps the Review page fast with a huge backlog.
const MAX_REVIEW_ROWS: usize = 5000;

/// Suggestions shown at most, biggest first, so a flood of made-up payees
/// can't bury the page.
const MAX_SUGGESTIONS: usize = 50;

const BILL_COLS: &str = "id, name, amount, account_id, paid_from_id, person_id, cadence, start_date, auto_pay, match_text, active, varies";

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
            varies: r.get(11)?,
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
    /// The long digit runs in them (`digit_ids`).
    ids: Vec<String>,
}

/// Within 10%, or $5 for small bills: a steady bill's amount. In i128:
/// imported amounts can sit at the edges of i64, and the difference mustn't
/// overflow and panic.
fn amount_close(bill: Cents, paid: Cents) -> bool {
    let slack = (bill.0 / STEADY_SLACK_DIVISOR).max(STEADY_MIN_SLACK.0);
    (i128::from(paid.0) - i128::from(bill.0)).abs() <= i128::from(slack)
}

/// Whether `paid` is about what the bill comes to.
fn amount_fits(bill: &NewRecurringBill, paid: Cents) -> bool {
    if bill.amount.is_zero() {
        // Not known: the category and match text do the work. Only the user
        // makes such a bill; a suggestion always carries an amount.
        return paid.0 > 0;
    }
    if bill.varies {
        let (typical, paid) = (i128::from(bill.amount.0), i128::from(paid.0));
        return paid * 100 >= typical * VARIES_MIN_PERCENT && paid * 100 <= typical * VARIES_MAX_PERCENT;
    }
    amount_close(bill.amount, paid)
}

/// What a bill's `match_text` points at: the payee text, and any ids a
/// payment must carry too ("ACH DEBIT 8005551212" isn't "ACH DEBIT 3334445555").
fn needle(match_text: Option<&str>) -> Option<(String, Vec<String>)> {
    let t = match_text?;
    let base = base_key(t);
    let ids = digit_ids(t);
    (!base.is_empty() || !ids.is_empty()).then_some((base, ids))
}

/// The payment that ticks off a bill's due date, if one landed: into the
/// bill's category, within `MATCH_DAYS`, about the right amount, carrying its
/// `match_text`, and not already claimed. The closest in date wins and is
/// claimed.
fn find_payment(bill: &NewRecurringBill, due: NaiveDate, candidates: &[Candidate], claimed: &mut HashSet<(EntryId, AccountId)>) -> Option<Payment> {
    let needle = needle(bill.match_text.as_deref());
    let found = candidates
        .iter()
        .filter(|c| {
            c.account_id == bill.account_id
                && (c.date - due).num_days().abs() <= MATCH_DAYS
                && amount_fits(bill, c.amount)
                && needle.as_ref().is_none_or(|(text, ids)| c.text.contains(text.as_str()) && ids.iter().all(|id| c.ids.contains(id)))
                && !claimed.contains(&(c.entry_id, c.account_id))
        })
        .min_by_key(|c| (c.date - due).num_days().abs())?;
    claimed.insert((found.entry_id, found.account_id));
    Some(Payment { entry_id: found.entry_id, date: found.date, amount: found.amount, manual: false })
}

/// A marked skip beats everything; then a payment; then the calendar.
fn due_state(skipped: bool, paid: bool, auto_pay: bool, due: NaiveDate, today: NaiveDate) -> DueState {
    if skipped {
        DueState::Skipped
    } else if paid {
        DueState::Paid
    } else if due == today {
        DueState::DueToday
    } else if due > today {
        DueState::Upcoming
    } else if auto_pay && (today - due).num_days() <= AUTO_PAY_GRACE_DAYS {
        DueState::Processing
    } else {
        DueState::Overdue
    }
}

/// One payment seen for a would-be bill, from the books or from Review.
struct SeenPayment {
    date: NaiveDate,
    /// Money out, positive.
    amount: Cents,
    /// The payee as the bank or memo named it (`payee_name`), not yet tidied.
    name: String,
    category: Option<AccountId>,
    paid_from: Option<AccountId>,
    person: Option<PersonId>,
    in_review: bool,
}

/// Payments by payee key without its ids (`payee_base`), each with the long
/// ids it carried; `split_by_originator` sorts those out once it's known
/// which ones repeat.
type ByBase = HashMap<String, Vec<(Vec<String>, SeenPayment)>>;

/// Payments by full payee key. An id on more than one payment names who's
/// charging (an ACH originator), so it's part of the payee: a different
/// originator behind the same bank text is a different payee. An id seen
/// once is a reference number and says nothing.
fn split_by_originator(groups: ByBase) -> HashMap<String, Vec<SeenPayment>> {
    let mut payees: HashMap<String, Vec<SeenPayment>> = HashMap::new();
    for (base, rows) in groups {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for id in rows.iter().flat_map(|(ids, _)| ids) {
            *counts.entry(id.clone()).or_default() += 1;
        }
        for (ids, seen) in rows {
            let own = ids.iter().filter(|id| counts[id.as_str()] > 1).map(String::as_str);
            payees.entry(with_ids(&base, own)).or_default().push(seen);
        }
    }
    payees
}

/// Whether an existing bill already covers a suggested payee (`base`, its
/// `ids`), so it needn't be suggested. Only a bill that would tick this
/// payee's payments off counts: the amount must fit too, or a look-alike
/// charging five times as much ("NETFLIX.COM-PREMIUM") hides behind Netflix.
fn bill_covers(bill: &NewRecurringBill, base: &str, ids: &[String], amount: Cents, category: Option<AccountId>) -> bool {
    let words = |t: &str| mentions(base, t) || mentions(t, base);
    let text = needle(bill.match_text.as_deref());
    amount_fits(bill, amount)
        // A bill pointed at one originator's id covers no other.
        && text.as_ref().is_none_or(|(_, t_ids)| t_ids.iter().all(|id| ids.contains(id)))
        // What the user pointed at, as whole words...
        && (text.as_ref().is_some_and(|(t, _)| words(t))
            // ...or the bill's name ("Gas" isn't every gas station, but the amount says)...
            || words(&base_key(&bill.name))
            // ...or, with nothing to go on, its category.
            || (text.is_none() && Some(bill.account_id) == category))
}

impl Store {
    /// Every bill, active first, with its next due date as of `today`.
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

    /// One bill and whether it's active, or `NotFound`.
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
        if b.amount > MAX_BILL_AMOUNT {
            return rule(format!("{} can't be more than ${MAX_BILL_AMOUNT}", b.name.trim()));
        }
        if b.varies && b.amount.is_zero() {
            return rule("give a typical amount for a bill that varies");
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

    /// Sets up a bill. It must be filed under a household spending category,
    /// and paid (if that's said) from a household bank account or card.
    pub fn create_recurring_bill(&self, b: &NewRecurringBill) -> Result<RecurringBillId> {
        self.validate_bill(b)?;
        insert_with_id(
            &self.conn,
            "recurring_bills",
            "INSERT INTO recurring_bills(name, amount, account_id, paid_from_id, person_id, cadence, start_date, auto_pay, match_text, varies, id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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
                b.varies
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Changes a bill, checked as `create_recurring_bill` checks a new one.
    /// Due dates already marked by hand keep their marks.
    pub fn update_recurring_bill(&self, id: RecurringBillId, b: &NewRecurringBill) -> Result<()> {
        self.validate_bill(b)?;
        self.recurring_bill(id)?;
        self.conn.execute(
            "UPDATE recurring_bills SET name = ?1, amount = ?2, account_id = ?3, paid_from_id = ?4, person_id = ?5,
                    cadence = ?6, start_date = ?7, auto_pay = ?8, match_text = ?9, varies = ?11 WHERE id = ?10",
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
                id,
                b.varies
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
                let s = if s == DueState::Paid { MARK_PAID } else { MARK_SKIPPED };
                self.conn.execute(
                    "INSERT INTO recurring_bill_marks(bill_id, due_date, status, entry_id) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(bill_id, due_date) DO UPDATE SET status = excluded.status, entry_id = excluded.entry_id",
                    params![id, due, s, entry_id.filter(|_| s == MARK_PAID)],
                )?;
            }
            Some(_) => return rule("a due date can only be marked paid or skipped"),
        }
        Ok(())
    }

    /// Every household payment into a spending category dated in `[from, to]`,
    /// one per entry and category, oldest first. Voids and reversals are left
    /// out: a voided payment never paid anything.
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
                Ok(Candidate {
                    entry_id: r.get(0)?,
                    date: r.get(1)?,
                    account_id: r.get(2)?,
                    amount: Cents(r.get(3)?),
                    text: normalize_description(&visible(&text)),
                    ids: digit_ids(&text),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Every due date of every active bill in `[from, to]`, with whether
    /// it's been paid, judged as of `today`.
    ///
    /// Due dates are settled oldest first, and each payment settles at most
    /// one, so two due dates close together can't both claim the same payment.
    pub fn due_dates(&self, from: NaiveDate, to: NaiveDate, today: NaiveDate) -> Result<Vec<DueDate>> {
        let bills = self.recurring_bills(today)?;
        let lookback = from - Days::new(CLAIMED_LOOKBACK_DAYS);
        let candidates = self.candidates(lookback - Days::new(MATCH_DAYS as u64), to + Days::new(MATCH_DAYS as u64))?;
        // What the user marked by hand: (bill, due date) -> (`MARK_PAID` or
        // `MARK_SKIPPED`, the entry that paid it).
        let marks: HashMap<(RecurringBillId, NaiveDate), (String, Option<EntryId>)> = {
            let mut stmt = self.conn.prepare("SELECT bill_id, due_date, status, entry_id FROM recurring_bill_marks")?;
            stmt.query_map([], |r| Ok(((r.get(0)?, r.get(1)?), (r.get(2)?, r.get(3)?))))?.collect::<rusqlite::Result<_>>()?
        };
        // Payments already spoken for: first those marked by hand, then each
        // one found as it's found.
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
                Some((s, entry)) if s == MARK_PAID => Some(match entry.and_then(|e| candidates.iter().find(|c| c.entry_id == e)) {
                    Some(c) => Payment { entry_id: c.entry_id, date: c.date, amount: c.amount, manual: true },
                    // Marked paid with no entry, or one outside the window: take the bill's word for it.
                    None => Payment { entry_id: entry.unwrap_or_default(), date: due, amount: b.bill.amount, manual: true },
                }),
                Some(_) => None,
                None => find_payment(&b.bill, due, &candidates, &mut claimed),
            };
            // Earlier due dates were only settled to claim their payments.
            if due < from {
                continue;
            }
            let skipped = mark.is_some_and(|m| m.0 == MARK_SKIPPED);
            out.push(DueDate {
                bill_id: b.id,
                name: b.bill.name.clone(),
                amount: b.bill.amount,
                account_id: b.bill.account_id,
                person_id: b.bill.person_id,
                auto_pay: b.bill.auto_pay,
                due_date: due,
                state: due_state(skipped, payment.is_some(), b.bill.auto_pay, due, today),
                payment,
            });
        }
        Ok(out)
    }

    /// Payments that repeat on a schedule but aren't bills yet: the same
    /// payee, weekly, monthly, quarterly or yearly, for about the same amount
    /// (or a usage bill that moves around), over the last two years and still
    /// going. Money out that's still waiting in Review counts too, so a
    /// fresh bank import is enough to spot them.
    pub fn bill_suggestions(&self, today: NaiveDate) -> Result<Vec<BillSuggestion>> {
        let from = today - Days::new(LOOKBACK_DAYS);
        let mut groups = ByBase::new();
        self.gather_booked_payments(from, today, &mut groups)?;
        self.gather_review_payments(from, today, &mut groups)?;
        let payees = split_by_originator(groups);

        let dismissed: HashSet<String> = self.dismissed_bill_suggestions()?.into_iter().collect();
        let existing = self.recurring_bills(today)?;
        let mut out = Vec::new();
        for (key, mut seen) in payees {
            if dismissed.contains(&key) {
                continue;
            }
            let Some(p) = schedule::detect(&seen.iter().map(|s| (s.date, s.amount)).collect::<Vec<_>>(), today) else {
                continue;
            };
            // Too big to be a household bill, or to save as one.
            if p.amount > MAX_BILL_AMOUNT {
                continue;
            }
            seen.sort_by_key(|s| s.date);
            let account_id = most_common(seen.iter().filter_map(|s| s.category));
            let (base, ids) = (base_key(&key), digit_ids(&key));
            if existing.iter().any(|b| bill_covers(&b.bill, &base, &ids, p.amount, account_id)) {
                continue;
            }
            let Some(latest) = seen.last() else {
                continue;
            };
            out.push(BillSuggestion {
                name: display_name(&latest.name),
                account_id,
                amount: p.amount,
                varies: p.varies,
                cadence: p.cadence,
                start_date: p.start,
                last_seen: p.last,
                times_seen: p.times,
                in_review: seen.iter().filter(|s| s.in_review).count(),
                match_text: key,
                paid_from_id: seen.iter().rev().find_map(|s| s.paid_from),
                person_id: most_common(seen.iter().filter_map(|s| s.person)),
            });
        }
        // Biggest first: those matter most to plan around.
        out.sort_by(|a, b| b.amount.cmp(&a.amount).then_with(|| a.name.cmp(&b.name)).then_with(|| a.match_text.cmp(&b.match_text)));
        out.truncate(MAX_SUGGESTIONS);
        Ok(out)
    }

    /// Household spending already in the books, since `from`.
    fn gather_booked_payments(&self, from: NaiveDate, today: NaiveDate, groups: &mut ByBase) -> Result<()> {
        let mut stmt = self.conn.prepare(
            "SELECT e.date, l.account_id, l.amount, COALESCE(bt.description, e.memo), bt.merchant, bt.ledger_account_id, e.person_id
             FROM journal_lines l
             JOIN journal_entries e ON e.id = l.entry_id
             JOIN accounts a ON a.id = l.account_id
             LEFT JOIN bank_transactions bt ON bt.entry_id = e.id
             WHERE a.scope = 'household' AND a.subtype IN ('expense', 'other_expense') AND l.amount > 0
               AND e.date >= ?1 AND e.date <= ?2 AND e.voided_by_id IS NULL AND e.reverses_id IS NULL
             ORDER BY e.date, e.id, l.id",
        )?;
        let rows = stmt.query_map(params![from, today], |r| {
            let description: String = r.get(3)?;
            let merchant: Option<String> = r.get(4)?;
            Ok((
                description.clone(),
                SeenPayment {
                    date: r.get(0)?,
                    category: Some(r.get(1)?),
                    amount: Cents(r.get(2)?),
                    name: payee_name(&description, merchant.as_deref()),
                    paid_from: r.get(5)?,
                    person: r.get(6)?,
                    in_review: false,
                },
            ))
        })?;
        for row in rows {
            let (description, seen) = row?;
            if let Some(base) = payee_base(&description) {
                groups.entry(base).or_default().push((digit_ids(&description), seen));
            }
        }
        Ok(())
    }

    /// Money out of a household account that's still waiting in Review,
    /// since `from`, with the category and person it would probably get.
    /// Rows that are already in the books, or aren't spending, are left out.
    fn gather_review_payments(&self, from: NaiveDate, today: NaiveDate, groups: &mut ByBase) -> Result<()> {
        let accounts: HashMap<AccountId, Account> = self.accounts()?.into_iter().map(|a| (a.id, a)).collect();
        let household = |id: AccountId| accounts.get(&id).filter(|a| a.scope == Scope::Household);
        let rules = self.rules()?;
        let review = self.review_money_out(from, today, MAX_REVIEW_ROWS)?;
        let bases: Vec<Option<String>> = review.iter().map(|t| payee_base(&t.description)).collect();
        let mut review_count: HashMap<&str, usize> = HashMap::new();
        for base in bases.iter().flatten() {
            *review_count.entry(base).or_default() += 1;
        }
        for (t, base) in review.iter().zip(&bases) {
            let Some(base) = base else {
                continue;
            };
            // Seen once, it can't be on a schedule: skip working out where it goes.
            if groups.get(base).map_or(0, Vec::len) + review_count.get(base.as_str()).copied().unwrap_or(0) < 2 {
                continue;
            }
            // Money out, as a positive amount; i64::MIN has no positive and isn't a real payment.
            let Some(amount) = t.amount.0.checked_neg().filter(|a| *a > 0) else {
                continue;
            };
            let category = match self.suggest(t, &rules)? {
                // Already in the books, and counted by `gather_booked_payments`.
                Some(Suggestion::Match { .. }) => continue,
                Some(Suggestion::Rule { account_id, .. } | Suggestion::History { account_id }) => {
                    if household(account_id).is_none_or(|a| a.kind() != AccountKind::Expense) {
                        // Paying off a card or moving money, not a bill.
                        continue;
                    }
                    Some(account_id)
                }
                None => None,
            };
            let seen = SeenPayment {
                date: t.date,
                amount: Cents(amount),
                name: payee_name(&t.description, t.merchant.as_deref()),
                category,
                paid_from: Some(t.ledger_account_id),
                person: self.suggest_person(t, &rules)?,
                in_review: true,
            };
            groups.entry(base.clone()).or_default().push((digit_ids(&t.description), seen));
        }
        Ok(())
    }

    /// Waved-off payees, oldest first.
    fn dismissed_bill_suggestions(&self) -> Result<Vec<String>> {
        Ok(crate::setting(&self.conn, DISMISSED_KEY)?
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default())
    }

    /// "Not a bill": stop suggesting this payee. Once `MAX_DISMISSED` are
    /// waved off, the oldest is forgotten (and may be suggested again), so
    /// this always works however many payees a bank import throws up.
    pub fn dismiss_bill_suggestion(&self, match_text: &str) -> Result<()> {
        if base_key(match_text).len() < MIN_PAYEE_LEN {
            return rule("that's too short to be a payee");
        }
        // The same key `bill_suggestions` makes, so a long one still matches.
        let key = payee_key(match_text);
        let mut list = self.dismissed_bill_suggestions()?;
        // Waved off again, it's the newest.
        list.retain(|k| *k != key);
        list.push(key);
        let excess = list.len().saturating_sub(MAX_DISMISSED);
        list.drain(..excess);
        crate::set_setting(&self.conn, DISMISSED_KEY, Some(&serde_json::to_string(&list).expect("strings serialize")))
    }
}

/// Bank text without invisible or direction-flipping characters: a U+202E
/// shows "xilfteN" as "Netflix". What's grouped is what's shown.
fn visible(raw: &str) -> String {
    raw.chars().filter(|c| !hidden(*c)).collect()
}

/// Control, zero-width and bidi-override characters.
fn hidden(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{00AD}' | '\u{061C}' | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'
        )
}

/// A payee as `normalize_description` writes it, digits and all dropped,
/// cut to `MAX_PAYEE_CHARS`.
fn base_key(raw: &str) -> String {
    truncate_chars(&normalize_description(&visible(raw)), MAX_PAYEE_CHARS)
}

/// The payee key for grouping, or `None` for text that can't be a bill's.
fn payee_base(raw: &str) -> Option<String> {
    let key = base_key(raw);
    // Moving money between accounts repeats too, but it isn't a bill.
    (key.len() >= MIN_PAYEE_LEN && !key.contains("transfer") && !key.contains("xfer")).then_some(key)
}

/// The long digit runs in bank text (`ID_DIGITS`), in order, each once.
fn digit_ids(raw: &str) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for run in raw.split(|c: char| !c.is_ascii_digit()).filter(|r| r.len() >= ID_DIGITS) {
        // ASCII digits: cutting by bytes is cutting by characters.
        let id = run[..run.len().min(MAX_ID_DIGITS)].to_string();
        if !ids.contains(&id) {
            ids.push(id);
        }
        if ids.len() == IDS_READ {
            break;
        }
    }
    ids
}

/// A payee key: the text, then up to `MAX_IDS` ids, sorted, e.g.
/// `"ach debit 8005551212"`. `payee_key` of a key gives it back.
fn with_ids<'a>(base: &str, ids: impl IntoIterator<Item = &'a str>) -> String {
    let mut ids: Vec<&str> = ids.into_iter().collect();
    ids.sort_unstable();
    ids.dedup();
    ids.truncate(MAX_IDS);
    let mut key = base.to_string();
    for id in ids {
        key.push(' ');
        key.push_str(id);
    }
    key
}

/// The key `bill_suggestions` would make for this text, ids included.
fn payee_key(raw: &str) -> String {
    let ids = digit_ids(raw);
    with_ids(&base_key(raw), ids.iter().map(String::as_str))
}

/// What to call a payee: the bank's merchant name when it's the payee the
/// description (which the key comes from) names, else the description, so a
/// payment can't be grouped as one payee and shown as another.
fn payee_name(description: &str, merchant: Option<&str>) -> String {
    match merchant {
        Some(m) if mentions(&base_key(description), &base_key(m)) => m.to_string(),
        _ => description.to_string(),
    }
}

/// At most `max` characters, cut on a character boundary.
fn truncate_chars(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => s[..i].trim_end().to_string(),
        None => s.to_string(),
    }
}

/// Bank text made readable: "NETFLIX.COM 866" becomes "Netflix.com". Text
/// that already has lowercase letters was typed by a person, so it keeps its case.
fn display_name(raw: &str) -> String {
    truncate_chars(&readable(&visible(raw)), MAX_NAME_CHARS)
}

/// Words without digits or `#` (store numbers, references), title-cased
/// when the bank was shouting, each `unmask`ed.
fn readable(raw: &str) -> String {
    let words: Vec<&str> = raw
        .split(|c: char| c.is_whitespace() || c == '*')
        .filter(|w| !w.is_empty() && !w.chars().any(|c| c.is_ascii_digit() || c == '#'))
        .collect();
    if words.is_empty() {
        return raw.trim().to_string();
    }
    let shouting = !raw.chars().any(char::is_lowercase);
    let title = |w: &str| {
        let mut cs = w.chars();
        cs.next().map(|f| f.to_uppercase().chain(cs.flat_map(char::to_lowercase)).collect::<String>()).unwrap_or_default()
    };
    words.iter().map(|w| unmask(&if shouting { title(w) } else { (*w).to_string() })).collect::<Vec<_>>().join(" ")
}

/// A word mixing Latin letters with Cyrillic or Greek ones is a look-alike
/// ("NЕTFLIX" with a Cyrillic Е): the odd letters show as "?" so it can't
/// pass for the real payee. Words in one script are left alone.
fn unmask(word: &str) -> String {
    let lookalike = |c: char| matches!(c, '\u{0370}'..='\u{03FF}' | '\u{0400}'..='\u{052F}');
    if word.chars().any(|c| c.is_ascii_alphabetic()) && word.chars().any(lookalike) {
        word.chars().map(|c| if lookalike(c) { '?' } else { c }).collect()
    } else {
        word.to_string()
    }
}

/// Whether `name` appears in `text` as whole words.
fn mentions(text: &str, name: &str) -> bool {
    let word_edge = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric());
    name.len() >= MIN_PAYEE_LEN
        && text.match_indices(name).any(|(i, m)| word_edge(text[..i].chars().next_back()) && word_edge(text[i + m.len()..].chars().next()))
}

/// The value seen most often, if any.
fn most_common<T: Copy + Eq + std::hash::Hash>(xs: impl Iterator<Item = T>) -> Option<T> {
    let mut counts: HashMap<T, usize> = HashMap::new();
    let mut best: Option<(T, usize)> = None;
    for x in xs {
        let n = counts.entry(x).or_default();
        *n += 1;
        // Ties go to whichever was seen first.
        if best.is_none_or(|(_, m)| *n > m) {
            best = Some((x, *n));
        }
    }
    best.map(|(x, _)| x)
}

#[cfg(test)]
// Amounts are written as dollars_cents (e.g. 160_00), hence the grouping.
#[allow(clippy::inconsistent_digit_grouping)]
mod tests {
    use super::*;

    #[test]
    fn bank_text_is_capped_and_cut_on_character_boundaries() {
        let huge = "NETFLIX ".repeat(100_000);
        assert_eq!(display_name(&huge).chars().count(), MAX_NAME_CHARS - 1, "cut, then the trailing space trimmed");
        assert!(payee_key(&huge).chars().count() <= MAX_PAYEE_CHARS);
        // Multibyte text: never sliced mid-character.
        let wide = "é日🙂".repeat(200);
        assert_eq!(display_name(&wide).chars().count(), MAX_NAME_CHARS);
        assert_eq!(payee_key(&wide).chars().count(), MAX_PAYEE_CHARS);
        assert_eq!(display_name("CAFÉ İSTANBUL 0042"), "Café İstanbul");
        assert_eq!(display_name("  1234 ##  "), "1234 ##");
        assert_eq!(display_name(""), "");
    }

    #[test]
    fn mentions_handles_multibyte_neighbours() {
        assert!(mentions("café netflix 日本", "netflix"));
        assert!(!mentions("日netflix日", "netflix"));
        assert!(mentions("日 netflix", "netflix"));
        assert!(!mentions("anything", ""));
    }

    #[test]
    fn names_and_keys_drop_invisible_characters() {
        assert_eq!(display_name("\u{202E}xilfteN\u{200B} \u{202C}"), "xilfteN");
        assert_eq!(display_name("NET\u{200B}FLIX\u{FEFF}.COM\u{0007}"), "Netflix.com");
        assert_eq!(base_key("NET\u{200B}FLIX.COM"), base_key("NETFLIX.COM"), "what's grouped is what's shown");
        assert_eq!(display_name("NЕTFLIX.COM"), "N?tflix.com");
        assert_eq!(display_name("МОСКВА ΑΘΗΝΑ IKEA"), "Москва Αθηνα Ikea", "one script per word is fine");
    }

    #[test]
    fn payee_keys_keep_long_ids_and_round_trip() {
        assert_eq!(payee_key("ACH DEBIT 8005551212"), "ach debit 8005551212");
        assert_eq!(payee_key("SAFEWAY #1234 05/18"), "safeway /");
        let key = payee_key(&format!("ACH {} 99999999 11111111 {}", "1".repeat(40), "X".repeat(500)));
        assert_eq!(payee_key(&key), key);
        assert_eq!(digit_ids(&key).len(), MAX_IDS);
    }

    #[test]
    fn a_bill_that_varies_takes_half_to_double() {
        let power = NewRecurringBill {
            name: "Power".into(),
            amount: Cents(160_00),
            varies: true,
            account_id: 1,
            paid_from_id: None,
            person_id: None,
            cadence: Cadence::Month { day: 1 },
            start_date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            auto_pay: false,
            match_text: None,
        };
        assert!(amount_fits(&power, Cents(80_00)) && amount_fits(&power, Cents(320_00)));
        assert!(!amount_fits(&power, Cents(79_99)) && !amount_fits(&power, Cents(4_999_00)) && !amount_fits(&power, Cents(i64::MAX)));
        let steady = NewRecurringBill { varies: false, ..power.clone() };
        assert!(!amount_fits(&steady, Cents(200_00)));
        // Not known: anything that lands, as the user chose.
        assert!(amount_fits(&NewRecurringBill { amount: Cents::ZERO, ..power }, Cents(4_999_00)));
    }

    #[test]
    fn amount_close_survives_extreme_amounts() {
        assert!(!amount_close(Cents(i64::MAX), Cents(i64::MIN)));
        assert!(!amount_close(Cents(1_00), Cents(i64::MIN)));
        assert!(amount_close(Cents(i64::MAX), Cents(i64::MAX)));
    }
}
