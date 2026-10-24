//! Plaid client for a desktop app.
//!
//! A desktop app can't receive Plaid's OAuth redirects, so linking uses
//! **Hosted Link**: we create a link token with `hosted_link`, open its URL
//! in the system browser, then poll `/link/token/get` until the session
//! yields a public token. Transactions come from cursor-based
//! `/transactions/sync`.
//!
//! Self-hosters bring their own `client_id`/`secret` (Sandbox, or Limited
//! Production once Plaid approves them). Nothing is proxied through us.
//!
//! **Sign:** Plaid amounts are positive when money *leaves* the account.
//! Tallybook feeds are inflow-positive, so every amount is negated here and
//! nowhere else.

use crate::float_to_cents;
use chrono::NaiveDate;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tally_core::bank::{FeedAccount, FeedAccountKind, FeedBatch, FeedTransaction};

/// History asked for when a bank is first linked: two years, the most
/// Plaid's `transactions.days_requested` allows.
const HISTORY_DAYS_REQUESTED: u32 = 730;

/// Transactions per `/transactions/sync` page: Plaid's maximum.
const SYNC_PAGE_SIZE: u32 = 500;

/// Attempts at a full sync when Plaid keeps saying the data changed while
/// paging; after the last, the sync fails and asks to be tried again.
const SYNC_RESTARTS: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Environment {
    Sandbox,
    Production,
}

impl Environment {
    pub fn base_url(self) -> &'static str {
        match self {
            Environment::Sandbox => "https://sandbox.plaid.com",
            Environment::Production => "https://production.plaid.com",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub client_id: String,
    pub secret: String,
    pub environment: Environment,
}

#[derive(Debug, thiserror::Error)]
pub enum PlaidError {
    #[error("network error talking to Plaid: {0}")]
    Http(#[from] reqwest::Error),
    /// Plaid's structured error. `code` is e.g. `ITEM_LOGIN_REQUIRED`.
    #[error("Plaid {code}: {message}")]
    Api { error_type: String, code: String, message: String },
    #[error("unexpected Plaid response: {0}")]
    Decode(String),
}

impl PlaidError {
    /// The bank wants the user to log in again (password change, MFA expiry).
    pub fn needs_relink(&self) -> bool {
        matches!(self, PlaidError::Api { code, .. } if code == "ITEM_LOGIN_REQUIRED" || code == "PENDING_EXPIRATION")
    }

    fn is_mutation_during_pagination(&self) -> bool {
        matches!(self, PlaidError::Api { code, .. } if code == "TRANSACTIONS_SYNC_MUTATION_DURING_PAGINATION")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostedLink {
    pub link_token: String,
    pub url: String,
}

/// What a finished Link session gives us.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinkResult {
    pub public_token: String,
    pub institution_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkStatus {
    /// The user hasn't finished yet; poll again.
    Waiting,
    Done(LinkResult),
    /// The session ended without a bank being linked.
    Exited,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub access_token: String,
    pub item_id: String,
}

pub struct SyncResult {
    pub batch: FeedBatch,
    /// Plaid account id for each transaction id in `added`/`modified`.
    pub account_of: std::collections::HashMap<String, String>,
    pub next_cursor: String,
}

pub struct Client {
    http: reqwest::Client,
    mode: Mode,
    base_url: String,
}

enum Mode {
    /// The user's own Plaid keys, sent in each request body.
    Direct(Credentials),
    /// Tallybook Connect: no Plaid keys on this side; the paid plan's license
    /// key authenticates, and the server holds the keys and real access
    /// tokens. The "access token" this client sees is an opaque handle.
    Connect { license_key: String },
}

impl Client {
    pub fn new(creds: Credentials) -> Client {
        let base_url = creds.environment.base_url().to_string();
        Client { http: reqwest::Client::new(), mode: Mode::Direct(creds), base_url }
    }

    /// Connect mode: calls go to `{base_url}/v1/plaid/...` with the license key
    /// as a bearer token and no credentials in the body.
    pub fn via_connect(base_url: impl AsRef<str>, license_key: impl Into<String>) -> Client {
        Client {
            http: reqwest::Client::new(),
            mode: Mode::Connect { license_key: license_key.into() },
            base_url: format!("{}/v1/plaid", base_url.as_ref().trim_end_matches('/')),
        }
    }

    /// For tests against a local fake.
    pub fn with_base_url(mut self, url: impl Into<String>) -> Client {
        self.base_url = url.into();
        self
    }

    async fn call<T: DeserializeOwned>(&self, path: &str, mut body: Value) -> Result<T, PlaidError> {
        let request = self.http.post(format!("{}{path}", self.base_url));
        let request = match &self.mode {
            Mode::Direct(creds) => {
                body["client_id"] = json!(creds.client_id);
                body["secret"] = json!(creds.secret);
                request
            }
            Mode::Connect { license_key } => request.bearer_auth(license_key),
        };
        let resp = request.json(&body).send().await?;
        let status = resp.status();
        let value: Value = resp.json().await?;
        if !status.is_success() || value.get("error_code").is_some_and(|c| !c.is_null()) {
            return Err(api_error(&value));
        }
        serde_json::from_value(value).map_err(|e| PlaidError::Decode(format!("{path}: {e}")))
    }

    /// Starts a Hosted Link session. Pass `access_token` to repair an existing
    /// item (update mode) instead of adding a new one.
    pub async fn create_hosted_link(&self, user_id: &str, access_token: Option<&str>) -> Result<HostedLink, PlaidError> {
        let mut body = json!({
            "client_name": "Tallybook",
            "language": "en",
            "country_codes": ["US"],
            "user": { "client_user_id": user_id },
            "hosted_link": {},
        });
        match access_token {
            Some(token) => body["access_token"] = json!(token),
            None => {
                body["products"] = json!(["transactions"]);
                body["transactions"] = json!({ "days_requested": HISTORY_DAYS_REQUESTED });
            }
        }
        #[derive(Deserialize)]
        struct Resp {
            link_token: String,
            hosted_link_url: Option<String>,
        }
        let r: Resp = self.call("/link/token/create", body).await?;
        let url = r.hosted_link_url.ok_or_else(|| PlaidError::Decode("no hosted_link_url; is Hosted Link enabled for your Plaid account?".into()))?;
        Ok(HostedLink { link_token: r.link_token, url })
    }

    pub async fn link_status(&self, link_token: &str) -> Result<LinkStatus, PlaidError> {
        let v: Value = self.call("/link/token/get", json!({ "link_token": link_token })).await?;
        Ok(parse_link_status(&v))
    }

    pub async fn exchange_public_token(&self, public_token: &str) -> Result<Item, PlaidError> {
        self.call("/item/public_token/exchange", json!({ "public_token": public_token })).await
    }

    pub async fn accounts(&self, access_token: &str) -> Result<Vec<FeedAccount>, PlaidError> {
        let v: Value = self.call("/accounts/get", json!({ "access_token": access_token })).await?;
        parse_accounts(&v)
    }

    /// Drains `/transactions/sync` from `cursor` (empty = from the beginning).
    /// Restarts the whole drain if Plaid reports data changed mid-pagination,
    /// as its docs require.
    pub async fn sync(&self, access_token: &str, cursor: &str) -> Result<SyncResult, PlaidError> {
        'restart: for _ in 0..SYNC_RESTARTS {
            let mut result = SyncResult { batch: FeedBatch::default(), account_of: Default::default(), next_cursor: cursor.to_string() };
            loop {
                let mut body = json!({ "access_token": access_token, "count": SYNC_PAGE_SIZE });
                if !result.next_cursor.is_empty() {
                    body["cursor"] = json!(result.next_cursor);
                }
                let page: Value = match self.call("/transactions/sync", body).await {
                    Ok(v) => v,
                    Err(e) if e.is_mutation_during_pagination() => continue 'restart,
                    Err(e) => return Err(e),
                };
                let has_more = apply_sync_page(&page, &mut result)?;
                if !has_more {
                    return Ok(result);
                }
            }
        }
        Err(PlaidError::Decode("transactions kept changing during sync; try again shortly".into()))
    }

    /// Revokes the access token at Plaid (on disconnect).
    pub async fn remove_item(&self, access_token: &str) -> Result<(), PlaidError> {
        let _: Value = self.call("/item/remove", json!({ "access_token": access_token })).await?;
        Ok(())
    }
}

fn api_error(v: &Value) -> PlaidError {
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    PlaidError::Api {
        error_type: s("error_type"),
        code: if s("error_code").is_empty() { "UNKNOWN".into() } else { s("error_code") },
        message: if s("display_message").is_empty() { s("error_message") } else { s("display_message") },
    }
}

fn parse_link_status(v: &Value) -> LinkStatus {
    let Some(sessions) = v.get("link_sessions").and_then(Value::as_array) else {
        return LinkStatus::Waiting;
    };
    for session in sessions.iter().rev() {
        // Newer responses: results.item_add_results[]; older: on_success.
        if let Some(r) = session.pointer("/results/item_add_results/0")
            && let Some(token) = r.get("public_token").and_then(Value::as_str) {
                return LinkStatus::Done(LinkResult {
                    public_token: token.into(),
                    institution_name: r.pointer("/institution/name").and_then(Value::as_str).map(Into::into),
                });
            }
        if let Some(token) = session.pointer("/on_success/public_token").and_then(Value::as_str) {
            return LinkStatus::Done(LinkResult {
                public_token: token.into(),
                institution_name: session
                    .pointer("/on_success/metadata/institution/name")
                    .and_then(Value::as_str)
                    .map(Into::into),
            });
        }
        if session.get("on_exit").is_some_and(|e| !e.is_null()) || session.get("exit").is_some_and(|e| !e.is_null()) {
            return LinkStatus::Exited;
        }
    }
    LinkStatus::Waiting
}

fn parse_accounts(v: &Value) -> Result<Vec<FeedAccount>, PlaidError> {
    let accounts = v.get("accounts").and_then(Value::as_array).ok_or_else(|| PlaidError::Decode("no accounts".into()))?;
    accounts
        .iter()
        .map(|a| {
            let s = |p: &str| a.pointer(p).and_then(Value::as_str).map(str::to_string);
            let kind = match s("/type").as_deref() {
                Some("depository") => FeedAccountKind::Depository,
                Some("credit") => FeedAccountKind::Credit,
                Some("loan") => FeedAccountKind::Loan,
                Some("investment") => FeedAccountKind::Investment,
                _ => FeedAccountKind::Other,
            };
            Ok(FeedAccount {
                external_id: s("/account_id").ok_or_else(|| PlaidError::Decode("account without id".into()))?,
                name: s("/name").or_else(|| s("/official_name")).unwrap_or_else(|| "Account".into()),
                mask: s("/mask"),
                kind,
                // Plaid already reports credit balances as amount owed (natural).
                current_balance: a.pointer("/balances/current").and_then(Value::as_f64).and_then(float_to_cents),
                currency: s("/balances/iso_currency_code"),
            })
        })
        .collect()
}

fn parse_transaction(t: &Value) -> Result<(FeedTransaction, String), PlaidError> {
    let s = |k: &str| t.get(k).and_then(Value::as_str).map(str::to_string);
    let id = s("transaction_id").ok_or_else(|| PlaidError::Decode("transaction without id".into()))?;
    let date = s("date")
        .and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok())
        .ok_or_else(|| PlaidError::Decode(format!("transaction {id} has no date")))?;
    let plaid_amount = t
        .get("amount")
        .and_then(Value::as_f64)
        .and_then(float_to_cents)
        .ok_or_else(|| PlaidError::Decode(format!("transaction {id} has no amount")))?;
    let txn = FeedTransaction {
        external_id: id,
        date,
        amount: -plaid_amount,
        description: s("name").or_else(|| s("merchant_name")).unwrap_or_default(),
        merchant: s("merchant_name"),
        pending: t.get("pending").and_then(Value::as_bool).unwrap_or(false),
        category_hint: t.pointer("/personal_finance_category/detailed").and_then(Value::as_str).map(str::to_string),
    };
    Ok((txn, s("account_id").unwrap_or_default()))
}

/// Folds one sync page into `result`. Returns `has_more`.
fn apply_sync_page(page: &Value, result: &mut SyncResult) -> Result<bool, PlaidError> {
    let list = |k: &str| page.get(k).and_then(Value::as_array).cloned().unwrap_or_default();
    for t in list("added") {
        let (txn, account) = parse_transaction(&t)?;
        result.account_of.insert(txn.external_id.clone(), account);
        result.batch.added.push(txn);
    }
    for t in list("modified") {
        let (txn, account) = parse_transaction(&t)?;
        result.account_of.insert(txn.external_id.clone(), account);
        result.batch.modified.push(txn);
    }
    for t in list("removed") {
        if let Some(id) = t.get("transaction_id").and_then(Value::as_str) {
            result.batch.removed.push(id.to_string());
        }
    }
    result.next_cursor = page
        .get("next_cursor")
        .and_then(Value::as_str)
        .ok_or_else(|| PlaidError::Decode("sync page without next_cursor".into()))?
        .to_string();
    Ok(page.get("has_more").and_then(Value::as_bool).unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tally_core::money::Cents;

    #[test]
    fn sync_page_flips_sign_and_tracks_accounts() {
        let page = json!({
            "added": [
                {"transaction_id": "t1", "account_id": "a1", "amount": 12.34, "date": "2026-02-01",
                 "name": "UBER *TRIP", "merchant_name": "Uber", "pending": false,
                 "personal_finance_category": {"primary": "TRANSPORTATION", "detailed": "TRANSPORTATION_TAXIS_AND_RIDE_SHARES"}},
                {"transaction_id": "t2", "account_id": "a1", "amount": -2500, "date": "2026-02-02", "name": "ACH CREDIT"}
            ],
            "modified": [],
            "removed": [{"transaction_id": "t0", "account_id": "a1"}],
            "next_cursor": "c2",
            "has_more": false
        });
        let mut r = SyncResult { batch: FeedBatch::default(), account_of: Default::default(), next_cursor: String::new() };
        assert!(!apply_sync_page(&page, &mut r).unwrap());
        assert_eq!(r.batch.added[0].amount, Cents(-1234), "a purchase is an outflow");
        assert_eq!(r.batch.added[1].amount, Cents(250000), "a deposit is an inflow");
        assert_eq!(r.batch.added[0].merchant.as_deref(), Some("Uber"));
        assert_eq!(r.batch.removed, vec!["t0"]);
        assert_eq!(r.account_of["t2"], "a1");
        assert_eq!(r.next_cursor, "c2");
    }

    #[test]
    fn float_amounts_round_to_the_cent() {
        let page = json!({"added": [{"transaction_id": "x", "amount": 0.1 + 0.2, "date": "2026-01-01", "name": "x"}],
                          "next_cursor": "c", "has_more": true});
        let mut r = SyncResult { batch: FeedBatch::default(), account_of: Default::default(), next_cursor: String::new() };
        assert!(apply_sync_page(&page, &mut r).unwrap());
        assert_eq!(r.batch.added[0].amount, Cents(-30));
    }

    #[test]
    fn link_status_reads_both_response_shapes() {
        assert_eq!(parse_link_status(&json!({"link_sessions": []})), LinkStatus::Waiting);
        let new_shape = json!({"link_sessions": [{"results": {"item_add_results": [
            {"public_token": "public-sandbox-1", "institution": {"name": "First Platypus Bank"}}]}}]});
        assert_eq!(
            parse_link_status(&new_shape),
            LinkStatus::Done(LinkResult { public_token: "public-sandbox-1".into(), institution_name: Some("First Platypus Bank".into()) })
        );
        let old_shape = json!({"link_sessions": [{"on_success": {"public_token": "p2", "metadata": {}}}]});
        assert!(matches!(parse_link_status(&old_shape), LinkStatus::Done(r) if r.public_token == "p2"));
        let exited = json!({"link_sessions": [{"on_exit": {"error": null}}]});
        assert_eq!(parse_link_status(&exited), LinkStatus::Exited);
    }

    #[test]
    fn accounts_map_kinds_and_balances() {
        let v = json!({"accounts": [
            {"account_id": "a1", "name": "Plaid Checking", "mask": "0000", "type": "depository",
             "balances": {"current": 110.0, "iso_currency_code": "USD"}},
            {"account_id": "a2", "name": "Plaid Credit Card", "mask": "3333", "type": "credit",
             "balances": {"current": 410.0}}
        ]});
        let a = parse_accounts(&v).unwrap();
        assert_eq!(a[0].kind, FeedAccountKind::Depository);
        assert_eq!(a[1].current_balance, Some(Cents(41000)));
    }

    #[test]
    fn errors_are_structured() {
        let e = api_error(&json!({"error_type": "ITEM_ERROR", "error_code": "ITEM_LOGIN_REQUIRED",
                                  "error_message": "the login details of this item have changed", "display_message": null}));
        assert!(e.needs_relink());
        assert!(e.to_string().contains("login details"));
    }
}
