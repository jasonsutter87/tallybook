//! Customers and vendors.
//!
//! Methods: `contacts.*`.

use crate::{ApiResult, App, Archive, Id, params, unknown_method};
use serde::Deserialize;
use serde_json::Value;
use tally_store::*;

#[derive(Deserialize)]
struct UpdateContact {
    id: ContactId,
    contact: NewContact,
}

impl App {
    pub(crate) fn contacts(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "contacts.list" => self.with(|s| s.contacts()),
            "contacts.get" => {
                let p: Id = params(p)?;
                self.with(|s| s.contact(p.id))
            }
            "contacts.create" => {
                let p: NewContact = params(p)?;
                self.with(|s| s.create_contact(&p))
            }
            "contacts.update" => {
                let p: UpdateContact = params(p)?;
                self.with(|s| s.update_contact(p.id, &p.contact))
            }
            "contacts.archive" => {
                let p: Archive = params(p)?;
                self.with(|s| s.set_contact_archived(p.id, p.archived))
            }
            _ => Err(unknown_method(method)),
        }
    }
}
