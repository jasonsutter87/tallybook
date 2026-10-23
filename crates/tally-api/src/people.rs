//! People and monthly budgets (household side only).
//!
//! People are who money was for; budgets are monthly plans of ceilings
//! (spend at most) and floors (at least).
//!
//! Methods: `people.*`, `budget.month`, `budget.set`, `budget.remove`,
//! `budget.set_available`.

use crate::{ApiResult, App, Archive, MonthParam, params, today, unknown_method};
use serde::Deserialize;
use serde_json::Value;
use tally_core::Cents;
use tally_store::*;

/// `people.update`: replaces the person's name and color.
#[derive(Deserialize)]
struct UpdatePerson {
    id: PersonId,
    person: NewPerson,
}

/// `budget.set`: sets one line of `month`'s plan. Plans carry forward, so
/// this also changes later months that had no plan of their own.
#[derive(Deserialize)]
struct SetBudget {
    month: String,
    line: BudgetLine,
}

/// `budget.remove`: drops one line from `month`'s plan onward.
/// `target` is `{"kind": "person" | "account", "id": n}`.
#[derive(Deserialize)]
struct RemoveBudget {
    month: String,
    target: BudgetTarget,
}

/// `budget.set_available`: what there is to work with in `month`; null clears it.
#[derive(Deserialize)]
struct SetAvailable {
    month: String,
    amount: Option<Cents>,
}

impl App {
    pub(crate) fn people(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            // Household side only. People are who money was for; budgets are
            // monthly plans of ceilings (spend at most) and floors (at least).

            // null -> [Person], archived ones last.
            "people.list" => self.with(|s| s.people()),
            // {name, color?} -> new person id.
            "people.create" => {
                let p: NewPerson = params(p)?;
                self.with(|s| s.create_person(&p))
            }
            // {id, person} -> null.
            "people.update" => {
                let p: UpdatePerson = params(p)?;
                self.with(|s| s.update_person(p.id, &p.person))
            }
            // {id, archived} -> null. Archived people drop out of later plans.
            "people.archive" => {
                let p: Archive = params(p)?;
                self.with(|s| s.set_person_archived(p.id, p.archived))
            }
            // {month?} -> MonthBudget: the plan in effect, progress so far, and the
            // pace projection, judged as of today.
            "budget.month" => {
                let p: MonthParam = params(p)?;
                let month = p.month.unwrap_or_else(|| month_of(today()));
                self.with(|s| s.month_budget(&month, today()))
            }
            // {month, line} -> budget line id.
            "budget.set" => {
                let p: SetBudget = params(p)?;
                self.with(|s| s.set_budget_line(&p.month, &p.line))
            }
            // {month, target} -> null.
            "budget.remove" => {
                let p: RemoveBudget = params(p)?;
                self.with(|s| s.remove_budget_line(&p.month, p.target))
            }
            // {month, amount} -> null.
            "budget.set_available" => {
                let p: SetAvailable = params(p)?;
                self.with(|s| s.set_available(&p.month, p.amount))
            }
            _ => Err(unknown_method(method)),
        }
    }
}
