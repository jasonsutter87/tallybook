//! Reconciling an account against a statement.
//!
//! Methods: `reconcile.*`.

use crate::{ApiResult, App, Id, params, unknown_method};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::Value;
use tally_core::{AccountId, Cents};
use tally_store::*;

#[derive(Deserialize)]
struct StartRec {
    account_id: AccountId,
    statement_date: NaiveDate,
    statement_balance: Cents,
}

#[derive(Deserialize)]
struct Toggle {
    id: ReconciliationId,
    line_id: LineId,
    checked: bool,
}

#[derive(Deserialize)]
struct ToggleMany {
    id: ReconciliationId,
    line_ids: Vec<LineId>,
    checked: bool,
}

#[derive(Deserialize)]
struct AccountOnly {
    account_id: AccountId,
}

impl App {
    pub(crate) fn reconcile(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "reconcile.active" => {
                let p: AccountOnly = params(p)?;
                self.with(|s| s.active_reconciliation(p.account_id))
            }
            "reconcile.history" => {
                let p: AccountOnly = params(p)?;
                self.with(|s| s.reconciliations(p.account_id))
            }
            "reconcile.start" => {
                let p: StartRec = params(p)?;
                self.with(|s| s.start_reconciliation(p.account_id, p.statement_date, p.statement_balance))
            }
            "reconcile.candidates" => {
                let p: Id = params(p)?;
                self.with(|s| s.reconcile_candidates(p.id))
            }
            "reconcile.toggle" => {
                let p: Toggle = params(p)?;
                self.with(|s| s.set_cleared(p.id, p.line_id, p.checked))
            }
            "reconcile.toggle_many" => {
                let p: ToggleMany = params(p)?;
                self.with(|s| p.line_ids.iter().try_for_each(|line| s.set_cleared(p.id, *line, p.checked)))
            }
            "reconcile.summary" => {
                let p: Id = params(p)?;
                self.with(|s| s.reconciliation_summary(p.id))
            }
            "reconcile.finish" => {
                let p: Id = params(p)?;
                self.with(|s| s.finish_reconciliation(p.id))
            }
            "reconcile.cancel" => {
                let p: Id = params(p)?;
                self.with(|s| s.cancel_reconciliation(p.id))
            }
            "reconcile.undo_last" => {
                let p: AccountOnly = params(p)?;
                self.with(|s| s.undo_last_reconciliation(p.account_id))
            }
            _ => Err(unknown_method(method)),
        }
    }
}
