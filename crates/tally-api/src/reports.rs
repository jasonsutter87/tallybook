//! The dashboard and the financial reports.
//!
//! Methods: `dashboard`, `reports.pnl`, `reports.balance_sheet`,
//! `reports.trial_balance`, `reports.ar_aging`, `reports.ap_aging`.

use crate::{ApiResult, App, dashboard, main_side, params, today, unknown_method};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::Value;
use tally_core::Scope;

#[derive(Deserialize)]
struct ScopedRange {
    from: NaiveDate,
    to: NaiveDate,
    /// One side of the book; all of it when absent.
    scope: Option<Scope>,
}

#[derive(Deserialize)]
struct AsOf {
    as_of: Option<NaiveDate>,
    scope: Option<Scope>,
}

/// `dashboard`: which side to show; the book's main side when absent.
#[derive(Deserialize)]
struct MaybeScope {
    scope: Option<Scope>,
}

impl App {
    pub(crate) fn reports(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "dashboard" => {
                let p: MaybeScope = params(p)?;
                self.with(|s| {
                    let scope = match p.scope {
                        Some(scope) => scope,
                        None => main_side(s)?,
                    };
                    dashboard::build(s, today(), scope)
                })
            }
            "reports.pnl" => {
                let p: ScopedRange = params(p)?;
                self.with(|s| s.profit_and_loss_in(p.from, p.to, p.scope))
            }
            "reports.balance_sheet" => {
                let p: AsOf = params(p)?;
                self.with(|s| s.balance_sheet_in(p.as_of.unwrap_or_else(today), p.scope))
            }
            "reports.trial_balance" => {
                let p: AsOf = params(p)?;
                self.with(|s| s.trial_balance_in(p.as_of.unwrap_or_else(today), p.scope))
            }
            "reports.ar_aging" => {
                let p: AsOf = params(p)?;
                self.with(|s| s.ar_aging(p.as_of.unwrap_or_else(today)))
            }
            "reports.ap_aging" => {
                let p: AsOf = params(p)?;
                self.with(|s| s.ap_aging(p.as_of.unwrap_or_else(today)))
            }
            _ => Err(unknown_method(method)),
        }
    }
}
