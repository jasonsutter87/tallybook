//! What this copy of Tallybook is licensed for (`license.*`).
//!
//! Household books are free, with file import but no bank connections. Paid
//! plans are yearly Lemon Squeezy subscriptions, each including the ones below
//! it: Personal adds bank sync and the phone app, Business adds a business
//! side, and Business + Tax adds tax prep. A paid year of Business + Tax
//! covers the tax year it was paid in and the one before (so paying in
//! February to file last year's return works).
//!
//! When a plan ends, what it unlocked keeps working: the business side stays
//! and tax prep stays open for the years paid for. Only bank and phone sync
//! stop, and those are checked by the Connect server, not here.
//!
//! A key is checked online with Lemon Squeezy's public License API when it's
//! activated, and again only when the person asks (`license.refresh`), which
//! picks up a renewal or a plan change. The result is kept in `license.json`
//! in the app-config dir. It never locks anyone out of a book.

use crate::{ApiError, ApiResult, App, ok, params, today};
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;

const API: &str = "https://api.lemonsqueezy.com/v1/licenses";

/// The Lemon Squeezy store, from its dashboard. A key from any other store is refused.
const STORE_ID: u64 = 508747;
/// Each plan's Lemon Squeezy id, matched against a key's product id and its
/// variant id, so the plans can be separate products or variants of one.
/// A key that matches none of them is refused.
const PLANS: [(u64, Tier); 3] = [(1478035, Tier::Personal), (0, Tier::Business), (0, Tier::Tax)];
pub const PERSONAL_CHECKOUT: &str = "https://tallybook.lemonsqueezy.com/checkout/buy/b03763ac-fc25-4bea-aff2-2d1d6587d5cc";
pub const BUSINESS_CHECKOUT: &str = "https://tallybook.lemonsqueezy.com/buy/business";
pub const TAX_CHECKOUT: &str = "https://tallybook.lemonsqueezy.com/buy/business-tax";
/// Lemon Squeezy's customer portal: renew, change plan, update a card.
pub const MANAGE_URL: &str = "https://app.lemonsqueezy.com/my-orders";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Free,
    /// Bank sync and the phone app.
    Personal,
    Business,
    /// Business + Tax, for the years in [`License::tax_years`].
    Tax,
}

impl Tier {
    fn name(self) -> &'static str {
        match self {
            Tier::Free => "Free",
            Tier::Personal => "Personal",
            Tier::Business => "Business",
            Tier::Tax => "Business + Tax",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Activation {
    pub key: String,
    /// Lemon Squeezy's id for this activation, needed to release it.
    pub instance_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct License {
    pub tier: Tier,
    /// The most recently activated key.
    pub key: String,
    pub instance_id: String,
    pub email: Option<String>,
    pub activated_on: NaiveDate,
    /// Tax years covered, from every Business + Tax purchase activated here.
    /// Empty in licenses saved before tax years were tracked; see [`License::tax_years`].
    #[serde(default)]
    pub tax_years: Vec<i32>,
    /// Keys activated here before `key`, so removing the license frees them all.
    #[serde(default)]
    pub earlier: Vec<Activation>,
    /// Whether the plan was being paid for when last checked with Lemon
    /// Squeezy, and when. `None` in licenses saved before plans were yearly.
    #[serde(default)]
    pub active: Option<bool>,
    #[serde(default)]
    pub checked_on: Option<NaiveDate>,
}

impl License {
    /// The tax years this license covers, oldest first.
    pub fn tax_years(&self) -> Vec<i32> {
        if self.tier < Tier::Tax {
            vec![]
        } else if self.tax_years.is_empty() {
            season(self.activated_on.year())
        } else {
            self.tax_years.clone()
        }
    }
}

/// What a paid year of Business + Tax in `year` covers: that tax year and the one before.
fn season(year: i32) -> Vec<i32> {
    vec![year - 1, year]
}

/// `years` plus the season paid for in `year`, oldest first.
fn with_season(mut years: Vec<i32>, year: i32) -> Vec<i32> {
    years.extend(season(year));
    years.sort_unstable();
    years.dedup();
    years
}

/// The license after activating a key for `plan`, paid for in `paid`, on top of `old`.
/// The tier only ever goes up: a plan that ends keeps what it unlocked.
fn apply(old: Option<License>, plan: Tier, paid: i32, key: String, instance_id: String, email: Option<String>, on: NaiveDate) -> License {
    let mut tax_years = old.as_ref().map(License::tax_years).unwrap_or_default();
    if plan == Tier::Tax {
        tax_years = with_season(tax_years, paid);
    }
    let tier = plan.max(old.as_ref().map_or(Tier::Free, |o| o.tier));
    let mut earlier = old.as_ref().map(|o| o.earlier.clone()).unwrap_or_default();
    if let Some(o) = old.filter(|o| o.key != key) {
        earlier.push(Activation { key: o.key, instance_id: o.instance_id });
    }
    License { tier, key, instance_id, email, activated_on: on, tax_years, earlier, active: Some(true), checked_on: Some(on) }
}

/// The license after Lemon Squeezy reported the current key's plan on `on`:
/// `Some(plan)` while it's paid for, `None` once it has ended.
fn refreshed(mut l: License, plan: Option<Tier>, on: NaiveDate) -> License {
    if let Some(plan) = plan {
        l.tier = l.tier.max(plan);
        if plan == Tier::Tax {
            l.tax_years = with_season(l.tax_years(), on.year());
        }
    }
    l.active = Some(plan.is_some());
    l.checked_on = Some(on);
    l
}

#[derive(Serialize)]
struct Status {
    tier: Tier,
    name: &'static str,
    /// The key's last four characters, never the whole key.
    key_hint: Option<String>,
    email: Option<String>,
    activated_on: Option<NaiveDate>,
    /// Tax years covered, oldest first; empty without Business + Tax.
    tax_years: Vec<i32>,
    /// Whether the plan was paid up when last checked (`None`: never checked, or free).
    active: Option<bool>,
    checked_on: Option<NaiveDate>,
    /// Whether this copy's plan includes bank sync and the phone app. The
    /// Connect server checks the plan is still paid for on every sync.
    syncs: bool,
    personal_checkout: &'static str,
    business_checkout: &'static str,
    tax_checkout: &'static str,
    manage_url: &'static str,
}

#[derive(Deserialize)]
struct Activate {
    key: String,
}

fn file(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join("license.json")
}

pub(crate) fn load(config_dir: &Path) -> Option<License> {
    serde_json::from_slice(&std::fs::read(file(config_dir)).ok()?).ok()
}

pub(crate) fn save(config_dir: &Path, l: &License) -> ApiResult<()> {
    let json = serde_json::to_vec_pretty(l).map_err(|e| ApiError::new("internal", e.to_string()))?;
    crate::write_private(&file(config_dir), &json).map_err(|e| ApiError::new("io", e.to_string()))
}

/// The keychain entry holding the key of one activation.
fn key_entry(instance_id: &str) -> String {
    format!("license-key:{instance_id}")
}

/// A key's last four characters: all `license.json` keeps of a key whose
/// whole text is in the keychain.
fn hint(key: &str) -> String {
    key.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect()
}

/// Every (key, instance id) pair a license holds, the current one last.
fn activations(l: &mut License) -> impl Iterator<Item = (&mut String, &String)> {
    l.earlier.iter_mut().map(|a| (&mut a.key, &a.instance_id)).chain(std::iter::once((&mut l.key, &l.instance_id)))
}

/// The plan a Lemon Squeezy reply's key is for, matching `plans` against its
/// variant first, then its product. `None` for another store or an unknown plan.
fn plan_of(r: &Value, plans: &[(u64, Tier)]) -> Option<Tier> {
    let meta = &r["meta"];
    if meta["store_id"].as_u64() != Some(STORE_ID) {
        return None;
    }
    let find = |field: &str| meta[field].as_u64().and_then(|id| plans.iter().find(|(p, _)| *p == id).map(|&(_, t)| t));
    find("variant_id").or_else(|| find("product_id"))
}

/// The plan an activation reply grants, or why it grants nothing.
fn activated_plan(r: &Value, plans: &[(u64, Tier)]) -> ApiResult<Tier> {
    if r["activated"] != true {
        let why = r["error"].as_str().unwrap_or("Lemon Squeezy didn't accept that key");
        return Err(ApiError::new("license", why.to_string()));
    }
    match r["license_key"]["status"].as_str() {
        Some("disabled") => return Err(ApiError::new("license", "that key has been disabled (refunded?)")),
        Some("expired") => return Err(ApiError::new("license", "that plan has ended. Renew it, then activate the key again.")),
        _ => {}
    }
    plan_of(r, plans).ok_or_else(|| ApiError::new("license", "that isn't a Tallybook key"))
}

/// The plan a validation reply says is paid for now: `Ok(None)` once it has
/// ended. A reply that isn't about this store's plans is an error, never a lapse.
fn validated_plan(r: &Value, plans: &[(u64, Tier)]) -> ApiResult<Option<Tier>> {
    if r["license_key"].is_null() {
        let why = r["error"].as_str().unwrap_or("Lemon Squeezy didn't recognize that key");
        return Err(ApiError::new("license", why.to_string()));
    }
    let plan = plan_of(r, plans).ok_or_else(|| ApiError::new("license", "that isn't a Tallybook key"))?;
    let paid = r["valid"] == true && r["license_key"]["status"] == "active";
    Ok(paid.then_some(plan))
}

async fn post(action: &str, form: &[(&str, &str)]) -> ApiResult<Value> {
    let res = reqwest::Client::new()
        .post(format!("{API}/{action}"))
        .header("accept", "application/json")
        .form(form)
        .send()
        .await
        .map_err(|_| ApiError::new("offline", "couldn't reach Lemon Squeezy — check your connection and try again"))?;
    // Refusals come back as 4xx with the same JSON shape, so read the body either way.
    res.json().await.map_err(|e| ApiError::new("license", format!("unexpected reply from Lemon Squeezy: {e}")))
}

/// "2025–2026", or "2026" for one year.
fn span(years: &[i32]) -> String {
    match (years.first(), years.last()) {
        (Some(a), Some(b)) if a != b => format!("{a}–{b}"),
        (Some(a), _) => a.to_string(),
        _ => String::new(),
    }
}

impl App {
    pub(crate) fn tier(&self) -> Tier {
        load(&self.config_dir).map_or(Tier::Free, |l| l.tier)
    }

    /// Tax years this copy can prepare, oldest first.
    pub(crate) fn tax_years_licensed(&self) -> Vec<i32> {
        load(&self.config_dir).map(|l| l.tax_years()).unwrap_or_default()
    }

    /// Errors with code `license` unless this copy is licensed for `need`.
    pub(crate) fn require(&self, need: Tier) -> ApiResult<()> {
        if self.tier() >= need {
            return Ok(());
        }
        let what = match need {
            Tier::Tax => "Tax prep is part of Tallybook Business + Tax",
            Tier::Business => "Keeping a business takes Tallybook Business",
            _ => "Connecting a bank takes a Tallybook plan",
        };
        Err(ApiError::new("license", format!("{what}. Add your license key in Settings → License.")))
    }

    /// Errors with code `license` unless tax prep for `year` is licensed.
    pub(crate) fn require_tax_year(&self, year: i32) -> ApiResult<()> {
        self.require(Tier::Tax)?;
        let years = self.tax_years_licensed();
        if years.contains(&year) {
            return Ok(());
        }
        Err(ApiError::new(
            "license",
            format!(
                "Your Business + Tax plan covers {}. Tax prep for {year} needs this year's plan: renew it, then choose Check my plan in Settings → License.",
                span(&years)
            ),
        ))
    }

    /// `license.json` with each key read back from the keychain where it
    /// was put there (files from older versions hold whole keys).
    pub(crate) fn load_license(&self) -> Option<License> {
        let mut l = load(&self.config_dir)?;
        for (key, id) in activations(&mut l) {
            if let Ok(Some(full)) = self.secrets.get(&key_entry(id)) {
                *key = full;
            }
        }
        Some(l)
    }

    /// Saves a license with its keys in the keychain (`license-key:<instance
    /// id>`) and only their last four characters in `license.json`. A key the
    /// keychain won't take stays whole in the file, which only this user can read.
    pub(crate) fn save_license(&self, l: &License) -> ApiResult<()> {
        let mut on_disk = l.clone();
        for (key, id) in activations(&mut on_disk) {
            if !id.is_empty() && self.secrets.set(&key_entry(id), key).is_ok() {
                *key = hint(key);
            }
        }
        save(&self.config_dir, &on_disk)
    }

    pub(crate) async fn license(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "license.status" => {
                let l = load(&self.config_dir);
                let tier = l.as_ref().map_or(Tier::Free, |l| l.tier);
                let active = l.as_ref().and_then(|l| l.active);
                ok(Status {
                    tier,
                    name: tier.name(),
                    key_hint: l.as_ref().map(|l| hint(&l.key)),
                    email: l.as_ref().and_then(|l| l.email.clone()),
                    activated_on: l.as_ref().map(|l| l.activated_on),
                    tax_years: l.as_ref().map(License::tax_years).unwrap_or_default(),
                    active,
                    checked_on: l.as_ref().and_then(|l| l.checked_on),
                    syncs: tier >= Tier::Personal && active != Some(false),
                    personal_checkout: PERSONAL_CHECKOUT,
                    business_checkout: BUSINESS_CHECKOUT,
                    tax_checkout: TAX_CHECKOUT,
                    manage_url: MANAGE_URL,
                })
            }
            // {key} -> status. Activates the key for this computer, adding to what's already licensed.
            "license.activate" => {
                let key = params::<Activate>(p)?.key.trim().to_string();
                if key.is_empty() {
                    return Err(ApiError::new("bad_params", "paste your license key"));
                }
                let host = std::env::var("HOSTNAME").or_else(|_| std::env::var("COMPUTERNAME")).unwrap_or_else(|_| "Tallybook".into());
                let r = post("activate", &[("license_key", &key), ("instance_name", &host)]).await?;
                let plan = activated_plan(&r, &PLANS)?;
                let instance_id = r["instance"]["id"].as_str().unwrap_or_default().to_string();
                let email = r["meta"]["customer_email"].as_str().map(str::to_string);
                let license = apply(self.load_license(), plan, today().year(), key, instance_id, email, today());
                self.save_license(&license)?;
                Box::pin(self.license("license.status", Value::Null)).await
            }
            // -> status. Asks Lemon Squeezy whether the current key's plan is
            // still paid for, picking up a renewal (and its tax season) or a
            // plan change. A plan that has ended keeps everything but sync.
            "license.refresh" => {
                let Some(l) = self.load_license() else {
                    return Box::pin(self.license("license.status", Value::Null)).await;
                };
                let r = post("validate", &[("license_key", &l.key), ("instance_id", &l.instance_id)]).await?;
                let plan = validated_plan(&r, &PLANS)?;
                self.save_license(&refreshed(l, plan, today()))?;
                Box::pin(self.license("license.status", Value::Null)).await
            }
            // -> null. Frees every key activated here so they can move to another computer.
            "license.deactivate" => {
                if let Some(l) = self.load_license() {
                    // Best effort: forgetting the keys here matters more than telling Lemon Squeezy.
                    for a in l.earlier.iter().chain(std::iter::once(&Activation { key: l.key.clone(), instance_id: l.instance_id.clone() })) {
                        let _ = post("deactivate", &[("license_key", &a.key), ("instance_id", &a.instance_id)]).await;
                        let _ = self.secrets.delete(&key_entry(&a.instance_id));
                    }
                    let _ = std::fs::remove_file(file(&self.config_dir));
                }
                ok(json!(null))
            }
            _ => Err(crate::unknown_method(method)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Distinct ids for the plans; the shipped ones are placeholders until the store exists.
    const TEST_PLANS: [(u64, Tier); 3] = [(11, Tier::Personal), (22, Tier::Business), (33, Tier::Tax)];

    fn reply(product: u64, status: &str) -> Value {
        json!({
            "activated": true, "valid": true, "error": null,
            "license_key": { "status": status, "created_at": "2027-02-03T14:15:07.000000Z" },
            "instance": { "id": "abc" },
            "meta": { "store_id": STORE_ID, "product_id": product, "variant_id": 999, "customer_email": "alice@example.com" }
        })
    }

    fn day(y: i32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, 2, 3).unwrap()
    }

    fn k(s: &str) -> String {
        s.to_string()
    }

    #[test]
    fn activation_reads_the_plan_and_refuses_strangers() {
        assert_eq!(activated_plan(&reply(33, "active"), &TEST_PLANS).unwrap(), Tier::Tax);
        assert_eq!(activated_plan(&reply(11, "active"), &TEST_PLANS).unwrap(), Tier::Personal);

        // Plans sold as variants of one product match on the variant.
        let mut variant = reply(500, "active");
        variant["meta"]["variant_id"] = json!(22);
        assert_eq!(activated_plan(&variant, &TEST_PLANS).unwrap(), Tier::Business);

        let mut wrong_store = reply(33, "active");
        wrong_store["meta"]["store_id"] = json!(STORE_ID + 1);
        assert_eq!(activated_plan(&wrong_store, &TEST_PLANS).unwrap_err().message, "that isn't a Tallybook key");
        assert_eq!(activated_plan(&reply(44, "active"), &TEST_PLANS).unwrap_err().code, "license");
        assert!(activated_plan(&reply(33, "expired"), &TEST_PLANS).unwrap_err().message.contains("ended"));
        assert!(activated_plan(&reply(33, "disabled"), &TEST_PLANS).unwrap_err().message.contains("disabled"));
        let refused = json!({ "activated": false, "error": "This license key has reached the activation limit." });
        assert!(activated_plan(&refused, &TEST_PLANS).unwrap_err().message.contains("activation limit"));
        assert!(Tier::Tax > Tier::Business && Tier::Business > Tier::Personal && Tier::Personal > Tier::Free, "each plan includes the ones below");
    }

    #[test]
    fn a_paid_year_of_tax_covers_that_season_and_plans_only_go_up() {
        // Paid in February 2027: files 2026, estimates 2027.
        let tax = apply(None, Tier::Tax, 2027, k("T1"), k("i1"), None, day(2027));
        assert_eq!((tax.tier, tax.tax_years(), tax.active), (Tier::Tax, vec![2026, 2027], Some(true)));

        // Personal and Business cover no tax year; activating a lower plan never lowers the tier.
        let business = apply(None, Tier::Business, 2027, k("B1"), k("i3"), None, day(2027));
        assert_eq!((business.tier, business.tax_years()), (Tier::Business, vec![]));
        let both = apply(Some(tax), Tier::Personal, 2028, k("P1"), k("i4"), None, day(2028));
        assert_eq!((both.tier, both.tax_years()), (Tier::Tax, vec![2026, 2027]));
        assert_eq!(both.earlier, [Activation { key: k("T1"), instance_id: k("i1") }], "the first key can still be released");
    }

    #[test]
    fn checking_the_plan_picks_up_renewals_and_lapses_keep_what_was_paid_for() {
        let tax = apply(None, Tier::Tax, 2027, k("T1"), k("i1"), None, day(2027));

        // Renewed: a year later it's still active, so 2028's season is added.
        let renewed = refreshed(tax, validated_plan(&reply(33, "active"), &TEST_PLANS).unwrap(), day(2028));
        assert_eq!((renewed.tax_years(), renewed.active, renewed.checked_on), (vec![2026, 2027, 2028], Some(true), Some(day(2028))));

        // Lapsed: the business side and the paid-for tax years stay; only sync stops.
        let lapsed = refreshed(renewed, validated_plan(&reply(33, "expired"), &TEST_PLANS).unwrap(), day(2029));
        assert_eq!((lapsed.tier, lapsed.tax_years(), lapsed.active), (Tier::Tax, vec![2026, 2027, 2028], Some(false)));

        // An upgrade from Personal to Business on the same key raises the tier.
        let personal = apply(None, Tier::Personal, 2027, k("P1"), k("i2"), None, day(2027));
        let upgraded = refreshed(personal, validated_plan(&reply(22, "active"), &TEST_PLANS).unwrap(), day(2027));
        assert_eq!(upgraded.tier, Tier::Business);

        // A reply that isn't about this key is an error, never mistaken for a lapse.
        let unknown = json!({ "valid": false, "error": "license_key not found.", "license_key": null });
        assert_eq!(validated_plan(&unknown, &TEST_PLANS).unwrap_err().code, "license");
        let mut stranger = reply(33, "active");
        stranger["meta"]["store_id"] = json!(STORE_ID + 1);
        assert!(validated_plan(&stranger, &TEST_PLANS).is_err());
    }

    #[test]
    fn licenses_saved_before_yearly_plans_still_load() {
        let old: License = serde_json::from_value(json!({
            "tier": "tax", "key": "K", "instance_id": "i", "email": null, "activated_on": "2026-10-22"
        }))
        .unwrap();
        assert_eq!(old.tax_years(), [2025, 2026]);
        assert_eq!((old.active, old.checked_on), (None, None));
        assert_eq!(span(&old.tax_years()), "2025–2026");
        let business: License = serde_json::from_value(json!({
            "tier": "business", "key": "K", "instance_id": "i", "email": null, "activated_on": "2026-10-22", "tax_years": []
        }))
        .unwrap();
        assert_eq!(business.tier, Tier::Business);
    }
}
