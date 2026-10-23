//! Importing bank files (CSV, OFX) and bringing a Ledger book over.
//!
//! Methods: `import.csv_preview`, `import.csv`, `import.ofx_preview`,
//! `import.ofx`, `import.ledger_preview`, `import.ledger`.

use crate::{ApiError, ApiResult, App, OpenPath, ok, params, unknown_method};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use tally_core::AccountId;
use tally_feed::{csv_import, ofx};
use tally_store::*;

#[derive(Deserialize)]
struct Content {
    content: String,
}

#[derive(Deserialize)]
struct CsvImport {
    content: String,
    mapping: csv_import::CsvMapping,
    account_id: AccountId,
}

#[derive(Deserialize)]
struct OfxImport {
    content: String,
    index: usize,
    account_id: AccountId,
}

#[derive(Serialize)]
struct OfxPreview {
    account: tally_core::bank::FeedAccount,
    count: usize,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
}

/// `import.ledger`: the Ledger SQLite file to bring over.
/// Safe to repeat: only entries not imported before are added.
#[derive(Deserialize)]
struct LedgerImportParams {
    path: PathBuf,
    /// The household account Ledger's money lands in; a new
    /// "Ledger import" account when absent.
    money_account_id: Option<AccountId>,
}

impl App {
    pub(crate) fn import(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "import.csv_preview" => {
                let p: Content = params(p)?;
                csv_import::preview(p.content.as_bytes(), 8).map_err(|e| ApiError::new("rule", e.to_string())).and_then(ok)
            }
            "import.csv" => {
                let p: CsvImport = params(p)?;
                let txns = csv_import::parse(p.content.as_bytes(), &p.mapping).map_err(|e| ApiError::new("rule", e.to_string()))?;
                self.with(|s| s.import_feed(p.account_id, &tally_core::bank::FeedBatch { added: txns, ..Default::default() }))
            }
            "import.ofx_preview" => {
                let p: Content = params(p)?;
                let statements = ofx::parse(p.content.as_bytes()).map_err(|e| ApiError::new("rule", e.to_string()))?;
                ok(statements
                    .into_iter()
                    .map(|st| OfxPreview {
                        count: st.transactions.len(),
                        from: st.transactions.iter().map(|t| t.date).min(),
                        to: st.transactions.iter().map(|t| t.date).max(),
                        account: st.account,
                    })
                    .collect::<Vec<_>>())
            }
            "import.ofx" => {
                let p: OfxImport = params(p)?;
                let mut statements = ofx::parse(p.content.as_bytes()).map_err(|e| ApiError::new("rule", e.to_string()))?;
                if p.index >= statements.len() {
                    return Err(ApiError::new("bad_params", "no such statement in that file"));
                }
                let st = statements.swap_remove(p.index);
                self.with(|s| s.import_feed(p.account_id, &tally_core::bank::FeedBatch { added: st.transactions, ..Default::default() }))
            }

            // {path} -> LedgerPreview: counts and date range, without importing.
            "import.ledger_preview" => {
                let p: OpenPath = params(p)?;
                ok(Store::preview_ledger(&p.path)?)
            }
            // {path, money_account_id?} -> LedgerImport: what was added and skipped,
            // plus notes on anything that couldn't come over as-is.
            "import.ledger" => {
                let p: LedgerImportParams = params(p)?;
                self.with(|s| s.import_ledger(&p.path, p.money_account_id))
            }
            _ => Err(unknown_method(method)),
        }
    }
}
