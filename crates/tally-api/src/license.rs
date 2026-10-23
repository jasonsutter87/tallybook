//! What this copy of Tallybook is licensed for (`license.*`).
//!
//! Household books are free. Adding a business side takes a Business license,
//! which is yours for good. Tax prep takes Business + Tax, which includes
//! Business and is bought per tax season: a purchase covers the tax year it
//! was made in and the one before (so buying in February to file last year's
//! return works), and returning buyers renew for later years at a discount.
//! All sold through Lemon Squeezy.
//!
//! A key is checked online once, with Lemon Squeezy's public License API, and
//! the result is kept in `license.json` in the app-config dir; after that
//! nothing phones home. It never locks anyone out of a book, only out of
//! adding a business side or using tax prep for a year it doesn't cover.

use crate::{ApiError, ApiResult, App, ok, params, today};
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;

const API: &str = "https://api.lemonsqueezy.com/v1/licenses";

/// The Lemon Squeezy store and products, from its dashboard. A key from any
/// other store or product is refused.
const STORE_ID: u64 = 0;
const BUSINESS_PRODUCT_ID: u64 = 0;
const TAX_PRODUCT_ID: u64 = 0;
/// Next season's tax prep for someone who already bought Business + Tax.
const TAX_RENEWAL_PRODUCT_ID: u64 = 0;
pub const BUSINESS_CHECKOUT: &str = "https://tallybook.lemonsqueezy.com/buy/business";
pub const TAX_CHECKOUT: &str = "https://tallybook.lemonsqueezy.com/buy/business-tax";
pub const TAX_RENEWAL_CHECKOUT: &str = "https://tallybook.lemonsqueezy.com/buy/tax-renewal";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Free,
    Business,
    /// Business + Tax, for the years in [`License::tax_years`].
    Tax,
}

impl Tier {
    fn name(self) -> &'static str {
        match self {
            Tier::Free => "Household (free)",
            Tier::Business => "Business",
            Tier::Tax => "Business + Tax",
        }
    }
}

/// What a key was bought as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Product {
    Business,
    Tax,
    TaxRenewal,
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

/// What a purchase made in `year` covers: that tax year and the one before.
fn season(year: i32) -> Vec<i32> {
    vec![year - 1, year]
}

/// The license after activating `product` (bought in `bought`) on top of `old`.
fn apply(old: Option<License>, product: Product, bought: i32, key: String, instance_id: String, email: Option<String>, on: NaiveDate) -> ApiResult<License> {
    let mut tax_years = old.as_ref().map(License::tax_years).unwrap_or_default();
    if product == Product::TaxRenewal && tax_years.is_empty() {
        return Err(ApiError::new(
            "license",
            "that's a renewal key: activate your earlier Business + Tax key on this computer first, then this one",
        ));
    }
    if product != Product::Business {
        tax_years.extend(season(bought));
        tax_years.sort_unstable();
        tax_years.dedup();
    }
    let tier = match product {
        Product::Business => Tier::Business,
        Product::Tax | Product::TaxRenewal => Tier::Tax,
    }
    .max(old.as_ref().map_or(Tier::Free, |o| o.tier));
    let mut earlier = old.as_ref().map(|o| o.earlier.clone()).unwrap_or_default();
    if let Some(o) = old.filter(|o| o.key != key) {
        earlier.push(Activation { key: o.key, instance_id: o.instance_id });
    }
    Ok(License { tier, key, instance_id, email, activated_on: on, tax_years, earlier })
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
    business_checkout: &'static str,
    tax_checkout: &'static str,
    tax_renewal_checkout: &'static str,
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
    std::fs::create_dir_all(config_dir).map_err(|e| ApiError::new("io", e.to_string()))?;
    let json = serde_json::to_vec_pretty(l).map_err(|e| ApiError::new("internal", e.to_string()))?;
    std::fs::write(file(config_dir), json).map_err(|e| ApiError::new("io", e.to_string()))
}

/// What a Lemon Squeezy activation response is for and the year it was
/// bought, or why it grants nothing.
fn product_from(r: &Value) -> ApiResult<(Product, i32)> {
    if r["activated"] != true {
        let why = r["error"].as_str().unwrap_or("Lemon Squeezy didn't accept that key");
        return Err(ApiError::new("license", why.to_string()));
    }
    if r["license_key"]["status"] == "disabled" {
        return Err(ApiError::new("license", "that key has been disabled (refunded?)"));
    }
    let meta = &r["meta"];
    if meta["store_id"].as_u64() != Some(STORE_ID) {
        return Err(ApiError::new("license", "that isn't a Tallybook key"));
    }
    let product = match meta["product_id"].as_u64() {
        Some(id) if id == TAX_PRODUCT_ID => Product::Tax,
        Some(id) if id == TAX_RENEWAL_PRODUCT_ID => Product::TaxRenewal,
        Some(id) if id == BUSINESS_PRODUCT_ID => Product::Business,
        _ => return Err(ApiError::new("license", "that isn't a Tallybook key")),
    };
    // When the key was issued, e.g. "2027-02-03T14:15:07.000000Z".
    let bought = r["license_key"]["created_at"].as_str().and_then(|s| s.get(..4)?.parse().ok()).unwrap_or_else(|| today().year());
    Ok((product, bought))
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
        let what = if need == Tier::Tax { "Tax prep is part of Tallybook Business + Tax" } else { "Keeping a business takes Tallybook Business" };
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
            format!("Your Business + Tax license covers {}. Tax prep for {year} is a new season: renew in Settings → License.", span(&years)),
        ))
    }

    pub(crate) async fn license(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "license.status" => {
                let l = load(&self.config_dir);
                let tier = l.as_ref().map_or(Tier::Free, |l| l.tier);
                ok(Status {
                    tier,
                    name: tier.name(),
                    key_hint: l.as_ref().map(|l| l.key.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect()),
                    email: l.as_ref().and_then(|l| l.email.clone()),
                    activated_on: l.as_ref().map(|l| l.activated_on),
                    tax_years: l.as_ref().map(License::tax_years).unwrap_or_default(),
                    business_checkout: BUSINESS_CHECKOUT,
                    tax_checkout: TAX_CHECKOUT,
                    tax_renewal_checkout: TAX_RENEWAL_CHECKOUT,
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
                let (product, bought) = product_from(&r)?;
                let instance_id = r["instance"]["id"].as_str().unwrap_or_default().to_string();
                let email = r["meta"]["customer_email"].as_str().map(str::to_string);
                let license = apply(load(&self.config_dir), product, bought, key, instance_id, email, today())?;
                save(&self.config_dir, &license)?;
                Box::pin(self.license("license.status", Value::Null)).await
            }
            // -> null. Frees every key activated here so they can move to another computer.
            "license.deactivate" => {
                if let Some(l) = load(&self.config_dir) {
                    // Best effort: forgetting the keys here matters more than telling Lemon Squeezy.
                    for a in l.earlier.iter().chain(std::iter::once(&Activation { key: l.key.clone(), instance_id: l.instance_id.clone() })) {
                        let _ = post("deactivate", &[("license_key", &a.key), ("instance_id", &a.instance_id)]).await;
                    }
                    let _ = std::fs::remove_file(file(&self.config_dir));
                }
                ok(json!(null))
            }
            _ => Err(ApiError::new("unknown_method", format!("no such method: {method}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(product: u64, created: &str) -> Value {
        json!({
            "activated": true, "error": null,
            "license_key": { "status": "active", "created_at": created },
            "instance": { "id": "abc" },
            "meta": { "store_id": STORE_ID, "product_id": product, "customer_email": "alice@example.com" }
        })
    }

    fn day(y: i32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, 2, 3).unwrap()
    }

    #[test]
    fn activation_reads_the_purchase_year_and_refuses_strangers() {
        assert_eq!(product_from(&reply(TAX_PRODUCT_ID, "2027-02-03T14:15:07.000000Z")).unwrap().1, 2027);
        let mut wrong_store = reply(TAX_PRODUCT_ID, "2027-02-03T00:00:00Z");
        wrong_store["meta"]["store_id"] = json!(STORE_ID + 1);
        assert_eq!(product_from(&wrong_store).unwrap_err().code, "license");
        let refused = json!({ "activated": false, "error": "This license key has reached the activation limit." });
        assert!(product_from(&refused).unwrap_err().message.contains("activation limit"));
        assert!(Tier::Tax > Tier::Business && Tier::Business > Tier::Free, "Tax includes Business");
    }

    #[test]
    fn tax_is_per_season_and_business_is_for_good() {
        let k = |s: &str| s.to_string();
        // Bought in February 2027: files 2026, estimates 2027.
        let tax = apply(None, Product::Tax, 2027, k("T1"), k("i1"), None, day(2027)).unwrap();
        assert_eq!(tax.tax_years(), [2026, 2027]);
        assert_eq!(tax.tier, Tier::Tax);

        // A renewal the next season adds 2028 and keeps what was bought.
        let renewed = apply(Some(tax), Product::TaxRenewal, 2028, k("R1"), k("i2"), None, day(2028)).unwrap();
        assert_eq!(renewed.tax_years(), [2026, 2027, 2028]);
        assert_eq!(renewed.earlier, [Activation { key: k("T1"), instance_id: k("i1") }], "the first key can still be released");

        // A renewal needs a Business + Tax license to renew.
        let alone = apply(None, Product::TaxRenewal, 2028, k("R1"), k("i2"), None, day(2028));
        assert_eq!(alone.unwrap_err().code, "license");

        // Business never expires and covers no tax year; adding it never removes tax years.
        let business = apply(None, Product::Business, 2027, k("B1"), k("i3"), None, day(2027)).unwrap();
        assert_eq!((business.tier, business.tax_years()), (Tier::Business, vec![]));
        let both = apply(Some(renewed), Product::Business, 2029, k("B1"), k("i3"), None, day(2029)).unwrap();
        assert_eq!((both.tier, both.tax_years()), (Tier::Tax, vec![2026, 2027, 2028]));
    }

    #[test]
    fn licenses_saved_before_tax_years_cover_their_activation_season() {
        let old: License = serde_json::from_value(json!({
            "tier": "tax", "key": "K", "instance_id": "i", "email": null, "activated_on": "2026-10-22"
        }))
        .unwrap();
        assert_eq!(old.tax_years(), [2025, 2026]);
        assert_eq!(span(&old.tax_years()), "2025–2026");
    }
}
