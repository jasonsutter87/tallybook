//! Categorization rules for the inbox.
//!
//! Methods: `rules.*`.

use crate::{ApiResult, App, Id, params, unknown_method};
use serde::Deserialize;
use serde_json::Value;
use tally_core::rules::RuleId;
use tally_store::*;

#[derive(Deserialize)]
struct UpdateRule {
    id: RuleId,
    rule: NewRule,
}

impl App {
    pub(crate) fn rules(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "rules.list" => self.with(|s| s.rules()),
            "rules.create" => {
                let p: NewRule = params(p)?;
                self.with(|s| s.create_rule(&p))
            }
            "rules.update" => {
                let p: UpdateRule = params(p)?;
                self.with(|s| s.update_rule(p.id, &p.rule))
            }
            "rules.delete" => {
                let p: Id = params(p)?;
                self.with(|s| s.delete_rule(p.id))
            }
            _ => Err(unknown_method(method)),
        }
    }
}
