//! The dashboard and the financial reports.
//!
//! Methods: `dashboard`, `reports.pnl`, `reports.balance_sheet`,
//! `reports.trial_balance`, `reports.ar_aging`, `reports.ap_aging`.
//!
//! A/R and A/P are business-only by nature (invoices and bills have no
//! household side): their `scope` is honored by returning an empty report for
//! `household`.

use crate::{ApiResult, App, dashboard, main_side, params, today, unknown_method};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::Value;
use std::result::Result;
use tally_core::Scope;
use tally_core::invoice::AgingBuckets;
use tally_store::{AgingReport, StoreError};

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

/// Invoices, bills and contacts exist only on the business side, so A/R and
/// A/P aging has nothing to show for any other scope: asking for `household`
/// gets an empty report, while `business` or no scope covers everything.
fn business_only(scope: Option<Scope>) -> bool {
    matches!(scope, Some(scope) if scope != Scope::Business)
}

fn empty_aging(as_of: NaiveDate) -> Result<AgingReport, StoreError> {
    Ok(AgingReport { as_of, rows: vec![], totals: AgingBuckets::default() })
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
                let as_of = p.as_of.unwrap_or_else(today);
                self.with(|s| if business_only(p.scope) { empty_aging(as_of) } else { s.ar_aging(as_of) })
            }
            "reports.ap_aging" => {
                let p: AsOf = params(p)?;
                let as_of = p.as_of.unwrap_or_else(today);
                self.with(|s| if business_only(p.scope) { empty_aging(as_of) } else { s.ap_aging(as_of) })
            }
            _ => Err(unknown_method(method)),
        }
    }
}
