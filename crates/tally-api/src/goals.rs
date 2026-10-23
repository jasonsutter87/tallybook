//! Savings goals and the household insights built on them.
//!
//! Methods: `goals.*`, `entries.set_goal`, `household.alerts`,
//! `household.opportunities`, `household.score`, `household.wins`.

use crate::{ApiResult, App, Archive, MonthParam, params, today, unknown_method};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::Value;
use tally_core::{AccountId, Cents};
use tally_store::*;

/// `goals.update`: replaces every field of the goal.
#[derive(Deserialize)]
struct UpdateGoal {
    id: GoalId,
    goal: NewGoal,
}

/// `goals.contribute`: moves `amount` from account `from` into the goal's
/// savings account, tagged to the goal. Negative takes money back out.
#[derive(Deserialize)]
struct Contribute {
    id: GoalId,
    from: AccountId,
    amount: Cents,
    /// Today when absent.
    date: Option<NaiveDate>,
}

/// `entries.set_goal`: counts an existing entry toward a goal; null untags it.
/// The entry must touch the goal's savings account.
#[derive(Deserialize)]
struct SetGoal {
    id: tally_core::journal::EntryId,
    goal_id: Option<GoalId>,
}

impl App {
    pub(crate) fn goals(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            // null -> [Goal], each with saved so far and a projection.
            "goals.list" => self.with(|s| s.goals(today())),
            // NewGoal -> new goal id. The goal's account must be a household asset.
            "goals.create" => {
                let p: NewGoal = params(p)?;
                self.with(|s| s.create_goal(&p))
            }
            // {id, goal} -> null.
            "goals.update" => {
                let p: UpdateGoal = params(p)?;
                self.with(|s| s.update_goal(p.id, &p.goal))
            }
            // {id, archived} -> null.
            "goals.archive" => {
                let p: Archive = params(p)?;
                self.with(|s| s.set_goal_archived(p.id, p.archived))
            }
            // {id, from, amount, date?} -> {entry_id, finished}; finished means this one reached the target.
            "goals.contribute" => {
                let p: Contribute = params(p)?;
                self.with(|s| s.contribute_to_goal(p.id, p.from, p.amount, p.date.unwrap_or_else(today)))
            }
            // {id, goal_id} -> null.
            "entries.set_goal" => {
                let p: SetGoal = params(p)?;
                self.with(|s| s.set_entry_goal(p.id, p.goal_id))
            }
            // {month?} -> alerts: [Alert] (near or over budget, unpaid bills);
            // opportunities: Opportunities (spending by flex, where it could bend);
            // score: BudgetScore (a 300-850 style score with its factors);
            // wins: [Win] (true, specific good news).
            "household.alerts" | "household.opportunities" | "household.score" | "household.wins" => {
                let p: MonthParam = params(p)?;
                let month = p.month.unwrap_or_else(|| month_of(today()));
                match method {
                    "household.alerts" => self.with(|s| s.alerts(&month, today())),
                    "household.opportunities" => self.with(|s| s.opportunities(&month)),
                    "household.score" => self.with(|s| s.budget_score(&month, today())),
                    _ => self.with(|s| s.wins(&month, today())),
                }
            }
            _ => Err(unknown_method(method)),
        }
    }
}
