//! Invoices (money owed to the business).
//!
//! Methods: `invoices.*`.

use crate::{ApiResult, App, Id, IdDate, params, today, unknown_method};
use serde::Deserialize;
use serde_json::Value;
use tally_store::*;

#[derive(Deserialize)]
struct UpdateInvoice {
    id: InvoiceId,
    draft: InvoiceDraft,
}

impl App {
    pub(crate) fn invoices(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "invoices.list" => self.with(|s| s.invoices(today())),
            "invoices.get" => {
                let p: Id = params(p)?;
                self.with(|s| s.invoice(p.id, today()))
            }
            "invoices.create" => {
                let p: InvoiceDraft = params(p)?;
                self.with(|s| s.create_invoice(&p))
            }
            "invoices.update" => {
                let p: UpdateInvoice = params(p)?;
                self.with(|s| s.update_invoice(p.id, &p.draft))
            }
            "invoices.delete" => {
                let p: Id = params(p)?;
                self.with(|s| s.delete_draft_invoice(p.id))
            }
            "invoices.finalize" => {
                let p: Id = params(p)?;
                self.with(|s| s.finalize_invoice(p.id))
            }
            "invoices.void" => {
                let p: IdDate = params(p)?;
                self.with(|s| s.void_invoice(p.id, p.date.unwrap_or_else(today)))
            }
            _ => Err(unknown_method(method)),
        }
    }
}
