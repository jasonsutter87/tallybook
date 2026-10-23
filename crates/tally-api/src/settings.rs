//! Settings: preferences, fiscal year start and the books-closed lock date.
//!
//! Methods: `settings.get`, `settings.set`.
//!
//! `settings.set` writes only the preferences the Settings screen edits
//! ([`PREF_KEYS`]). Everything else kept in the same table has its own method
//! with its own checks: cloud copies (`cloud.set`), the tax profile
//! (`tax.set_profile`), bank links (`bank.*`).

use crate::{ApiError, ApiResult, App, params, unknown_method};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::Value;
// The glob above brings in the store's one-arg `Result`; keep std's.
use std::result::Result;

/// The preference keys `settings.set` accepts: the business profile and the
/// invoice defaults, as the Settings screen saves them.
pub(crate) const PREF_KEYS: &[&str] = &[
    "company.name",
    "company.email",
    "company.phone",
    "company.address",
    "invoice.next_number",
    "invoice.default_terms_days",
    "invoice.default_tax_bp",
    "invoice.footer",
];

#[derive(Deserialize)]
struct SetSettings {
    prefs: Option<std::collections::BTreeMap<String, Option<String>>>,
    fiscal_year_start_month: Option<u32>,
    /// `Some(None)` clears the lock date.
    #[serde(default, deserialize_with = "double_option")]
    lock_date: Option<Option<NaiveDate>>,
}

fn double_option<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<NaiveDate>>, D::Error> {
    Option::<NaiveDate>::deserialize(d).map(Some)
}

#[derive(Serialize)]
struct SettingsView {
    prefs: std::collections::BTreeMap<String, String>,
    fiscal_year_start_month: u32,
    lock_date: Option<NaiveDate>,
}

impl App {
    pub(crate) fn settings(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "settings.get" => self.with(|s| {
                Ok(SettingsView { prefs: s.prefs()?, fiscal_year_start_month: s.fiscal_year_start_month()?, lock_date: s.lock_date()? })
            }),
            "settings.set" => {
                let p: SetSettings = params(p)?;
                // Check every key before writing any, so a refused call changes nothing.
                if let Some(k) = p.prefs.iter().flat_map(|m| m.keys()).find(|k| !PREF_KEYS.contains(&k.as_str())) {
                    return Err(ApiError::new("bad_params", format!("{k} isn't a preference settings.set can change")));
                }
                self.with(|s| {
                    for (k, v) in p.prefs.unwrap_or_default() {
                        s.set_pref(&k, v.as_deref().map(str::trim).filter(|v| !v.is_empty()))?;
                    }
                    if let Some(m) = p.fiscal_year_start_month {
                        s.set_fiscal_year_start_month(m)?;
                    }
                    if let Some(lock) = p.lock_date {
                        s.set_lock_date(lock)?;
                    }
                    Ok(())
                })
            }
            _ => Err(unknown_method(method)),
        }
    }
}
