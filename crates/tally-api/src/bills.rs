//! Bills (money the business owes) and the payments that settle bills and invoices.
//!
//! Methods: `bills.*`, `payments.*`.

use crate::{ApiResult, App, Id, IdDate, params, today, unknown_method};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::{Value, json};
use tally_store::*;

#[derive(Deserialize)]
struct PaymentsQuery {
    kind: Option<PaymentKind>,
    contact_id: Option<ContactId>,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
}

impl App {
    pub(crate) fn bills(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "bills.list" => self.with(|s| s.bills(today())),
            "bills.get" => {
                let p: Id = params(p)?;
                self.with(|s| {
                    let (summary, lines, payments) = s.bill(p.id, today())?;
                    Ok(json!({ "summary": summary, "lines": lines, "payments": payments }))
                })
            }
            "bills.create" => {
                let p: NewBill = params(p)?;
                self.with(|s| s.create_bill(&p))
            }
            "bills.void" => {
                let p: IdDate = params(p)?;
                self.with(|s| s.void_bill(p.id, p.date.unwrap_or_else(today)))
            }
            "payments.receive" => {
                let p: NewPayment = params(p)?;
                self.with(|s| s.receive_payment(&p))
            }
            "payments.pay" => {
                let p: NewPayment = params(p)?;
                self.with(|s| s.pay_bills(&p))
            }
            "payments.list" => {
                let p: PaymentsQuery = params(p)?;
                let to = p.to.unwrap_or_else(today);
                let from = p.from.unwrap_or(NaiveDate::MIN);
                self.with(|s| s.payments(p.kind, p.contact_id, from, to))
            }
            "payments.void" => {
                let p: IdDate = params(p)?;
                self.with(|s| s.void_payment(p.id, p.date.unwrap_or_else(today)))
            }
            _ => Err(unknown_method(method)),
        }
    }
}
