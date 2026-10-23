//! Recurring bills: the schedule, due dates, and suggestions detected from the books.
//!
//! Methods: `recurring.*`.

use crate::{ApiResult, App, params, today, unknown_method};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::Value;
use std::result::Result;
use tally_store::*;

/// `recurring.update`: replaces every field of the bill.
#[derive(Deserialize)]
struct UpdateBill {
    id: RecurringBillId,
    bill: NewRecurringBill,
}

/// `recurring.set_active`: an inactive bill stops producing due dates.
#[derive(Deserialize)]
struct SetActive {
    id: RecurringBillId,
    active: bool,
}

/// `recurring.due`: the window of due dates to list. Defaults: from a week
/// ago through the end of next month.
#[derive(Deserialize)]
struct DueQuery {
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
}

/// `recurring.dismiss_suggestion`: "Not a bill". Send the suggestion's
/// `match_text`; it is remembered so the payee isn't suggested again.
#[derive(Deserialize)]
struct DismissSuggestion {
    match_text: String,
}

/// `recurring.mark`: overrides one due date by hand, e.g. paid in cash.
/// `due_date` must be one of the bill's actual due dates.
#[derive(Deserialize)]
struct MarkDue {
    bill_id: RecurringBillId,
    due_date: NaiveDate,
    /// "paid", "skipped", or null to go back to automatic.
    status: Option<DueState>,
    /// The entry that paid it, when there is one.
    entry_id: Option<tally_core::journal::EntryId>,
}

/// The window `recurring.due` lists when none is given: from a week before
/// `today` through the last day of next month. A calendar error is returned,
/// not papered over with some other date.
pub(crate) fn default_due_window(today: NaiveDate) -> Result<(NaiveDate, NaiveDate), StoreError> {
    let (_, month_end) = month_range(&month_of(today + chrono::Months::new(1)))?;
    Ok((today - chrono::Days::new(7), month_end))
}

impl App {
    pub(crate) fn recurring(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            // null -> [RecurringBill], each with its next due date.
            "recurring.list" => self.with(|s| s.recurring_bills(today())),
            // NewRecurringBill -> new bill id.
            "recurring.create" => {
                let p: NewRecurringBill = params(p)?;
                self.with(|s| s.create_recurring_bill(&p))
            }
            // {id, bill} -> null.
            "recurring.update" => {
                let p: UpdateBill = params(p)?;
                self.with(|s| s.update_recurring_bill(p.id, &p.bill))
            }
            // {id, active} -> null.
            "recurring.set_active" => {
                let p: SetActive = params(p)?;
                self.with(|s| s.set_recurring_bill_active(p.id, p.active))
            }
            // {from?, to?} -> [DueDate], each paid / overdue / upcoming / etc.
            "recurring.due" => {
                // Default: the past week through the end of next month.
                let p: DueQuery = params(p)?;
                let t = today();
                self.with(|s| {
                    let (default_from, default_to) = default_due_window(t)?;
                    s.due_dates(p.from.unwrap_or(default_from), p.to.unwrap_or(default_to), t)
                })
            }
            // {bill_id, due_date, status, entry_id?} -> null.
            "recurring.mark" => {
                let p: MarkDue = params(p)?;
                self.with(|s| s.mark_due_date(p.bill_id, p.due_date, p.status, p.entry_id))
            }
            // null -> [BillSuggestion]: repeat payments that look like bills,
            // detected from the books and the Review inbox. At most 50.
            "recurring.suggestions" => self.with(|s| s.bill_suggestions(today())),
            // {match_text} -> null.
            "recurring.dismiss_suggestion" => {
                let p: DismissSuggestion = params(p)?;
                self.with(|s| s.dismiss_bill_suggestion(&p.match_text))
            }
            _ => Err(unknown_method(method)),
        }
    }
}
