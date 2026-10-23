//! SimpleFIN Bridge client (https://beta-bridge.simplefin.org).
//!
//! The no-approval alternative to Plaid: the user buys access, pastes a
//! *setup token* (base64 of a one-time claim URL), and we trade it for a
//! long-lived *access URL* with credentials embedded. Amounts are already
//! inflow-positive.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, NaiveDate};
use serde::Deserialize;
use tally_core::bank::{FeedAccount, FeedAccountKind, FeedTransaction};
use tally_core::money::Cents;

#[derive(Debug, thiserror::Error)]
pub enum SimpleFinError {
    #[error("that doesn't look like a SimpleFIN setup token")]
    BadSetupToken,
    #[error("the setup token was already used or is invalid (HTTP {0})")]
    ClaimFailed(u16),
    #[error("network error talking to SimpleFIN: {0}")]
    Http(#[from] reqwest::Error),
    #[error("SimpleFIN access was revoked or the access URL is wrong (HTTP {0})")]
    Unauthorized(u16),
    #[error("SimpleFIN: {0}")]
    Bridge(String),
    #[error("unexpected SimpleFIN response: {0}")]
    Decode(String),
}

/// SimpleFIN Bridge's hosts. Claim and access URLs must point at one of
/// these (or a subdomain), over https, so bank credentials only ever go to
/// the bridge.
const BRIDGE_HOSTS: [&str; 2] = ["bridge.simplefin.org", "beta-bridge.simplefin.org"];

/// `url` parsed, if it is https to a SimpleFIN Bridge host.
fn bridge_url(url: &str) -> Option<reqwest::Url> {
    let url = reqwest::Url::parse(url.trim()).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    let ours = BRIDGE_HOSTS.iter().any(|h| host == *h || host.ends_with(&format!(".{h}")));
    (url.scheme() == "https" && ours).then_some(url)
}

/// Undoes URL percent-encoding (`%40` → `@`), as user names and passwords in
/// a URL are written.
fn percent_decode(s: &str) -> Option<String> {
    let mut out = Vec::with_capacity(s.len());
    let mut bytes = s.bytes();
    while let Some(b) = bytes.next() {
        if b == b'%' {
            let hex = [bytes.next()?, bytes.next()?];
            out.push(u8::from_str_radix(std::str::from_utf8(&hex).ok()?, 16).ok()?);
        } else {
            out.push(b);
        }
    }
    String::from_utf8(out).ok()
}

/// Decodes the setup token to its claim URL.
pub fn claim_url(setup_token: &str) -> Result<String, SimpleFinError> {
    let bytes = STANDARD.decode(setup_token.trim()).map_err(|_| SimpleFinError::BadSetupToken)?;
    let url = String::from_utf8(bytes).map_err(|_| SimpleFinError::BadSetupToken)?;
    bridge_url(&url).ok_or(SimpleFinError::BadSetupToken)?;
    Ok(url.trim().to_string())
}

/// Trades a setup token for the access URL. **One-shot**: store the result.
pub async fn claim(setup_token: &str) -> Result<String, SimpleFinError> {
    let url = claim_url(setup_token)?;
    let resp = reqwest::Client::new().post(url).header("Content-Length", "0").send().await?;
    if !resp.status().is_success() {
        return Err(SimpleFinError::ClaimFailed(resp.status().as_u16()));
    }
    let access_url = resp.text().await?.trim().to_string();
    // Never store an access URL that would send credentials anywhere else.
    split_access_url(&access_url)?;
    Ok(access_url)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleFinAccount {
    pub account: FeedAccount,
    pub institution: Option<String>,
    pub transactions: Vec<FeedTransaction>,
}

#[derive(Deserialize)]
struct AccountSet {
    #[serde(default)]
    errors: Vec<String>,
    accounts: Vec<RawAccount>,
}

#[derive(Deserialize)]
struct RawAccount {
    id: String,
    name: String,
    currency: Option<String>,
    balance: String,
    org: Option<RawOrg>,
    #[serde(default)]
    transactions: Vec<RawTxn>,
}

#[derive(Deserialize)]
struct RawOrg {
    name: Option<String>,
}

#[derive(Deserialize)]
struct RawTxn {
    id: String,
    posted: i64,
    amount: String,
    description: String,
    payee: Option<String>,
    #[serde(default)]
    pending: bool,
    transacted_at: Option<i64>,
}

fn unix_date(ts: i64) -> Option<NaiveDate> {
    DateTime::from_timestamp(ts, 0).map(|d| d.date_naive())
}

/// Splits `https://user:pass@host/path` into a bare URL and basic-auth
/// credentials (percent-decoded). Only https URLs to SimpleFIN Bridge with
/// both a user and a password are accepted.
fn split_access_url(access_url: &str) -> Result<(String, String, String), SimpleFinError> {
    let bad = || SimpleFinError::Decode("the access URL isn't an https SimpleFIN Bridge URL with credentials".into());
    let mut url = bridge_url(access_url).ok_or_else(bad)?;
    let user = percent_decode(url.username()).filter(|u| !u.is_empty()).ok_or_else(bad)?;
    let pass = url.password().and_then(percent_decode).filter(|p| !p.is_empty()).ok_or_else(bad)?;
    url.set_username("").map_err(|_| bad())?;
    url.set_password(None).map_err(|_| bad())?;
    Ok((url.to_string(), user, pass))
}

/// Fetches accounts and transactions posted on or after `since`.
pub async fn fetch(access_url: &str, since: NaiveDate) -> Result<Vec<SimpleFinAccount>, SimpleFinError> {
    let (base, user, pass) = split_access_url(access_url)?;
    let start = since.and_hms_opt(0, 0, 0).map(|d| d.and_utc().timestamp()).unwrap_or(0);
    let resp = reqwest::Client::new()
        .get(format!("{}/accounts", base.trim_end_matches('/')))
        .basic_auth(user, Some(pass))
        .query(&[("start-date", start.to_string()), ("pending", "1".into())])
        .send()
        .await?;
    let status = resp.status().as_u16();
    if status == 401 || status == 403 {
        return Err(SimpleFinError::Unauthorized(status));
    }
    if !resp.status().is_success() {
        return Err(SimpleFinError::Bridge(format!("HTTP {status}")));
    }
    parse_account_set(&resp.text().await?)
}

fn parse_account_set(body: &str) -> Result<Vec<SimpleFinAccount>, SimpleFinError> {
    let set: AccountSet = serde_json::from_str(body).map_err(|e| SimpleFinError::Decode(e.to_string()))?;
    if set.accounts.is_empty()
        && let Some(e) = set.errors.first()
    {
        return Err(SimpleFinError::Bridge(e.clone()));
    }
    set.accounts
        .into_iter()
        .map(|a| {
            let balance = Cents::parse(&a.balance).map_err(|e| SimpleFinError::Decode(e.to_string()))?;
            let transactions = a
                .transactions
                .into_iter()
                .map(|t| {
                    let ts = if t.posted > 0 { t.posted } else { t.transacted_at.unwrap_or(0) };
                    Ok(FeedTransaction {
                        external_id: format!("sfin:{}", t.id),
                        date: unix_date(ts).ok_or_else(|| SimpleFinError::Decode(format!("bad date on {}", t.id)))?,
                        amount: Cents::parse(&t.amount).map_err(|e| SimpleFinError::Decode(e.to_string()))?,
                        description: t.description,
                        merchant: t.payee,
                        pending: t.pending || t.posted == 0,
                        category_hint: None,
                    })
                })
                .collect::<Result<Vec<_>, SimpleFinError>>()?;
            // SimpleFIN doesn't type accounts; a negative balance is almost always a card or loan.
            let kind = if balance.0 < 0 { FeedAccountKind::Credit } else { FeedAccountKind::Depository };
            Ok(SimpleFinAccount {
                account: FeedAccount {
                    external_id: a.id,
                    name: a.name,
                    mask: None,
                    kind,
                    current_balance: Some(if kind == FeedAccountKind::Credit { -balance } else { balance }),
                    currency: a.currency,
                },
                institution: a.org.and_then(|o| o.name),
                transactions,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_setup_tokens() {
        let token = STANDARD.encode("https://beta-bridge.simplefin.org/simplefin/claim/abc");
        assert_eq!(claim_url(&token).unwrap(), "https://beta-bridge.simplefin.org/simplefin/claim/abc");
        assert!(claim_url("not base64!!").is_err());
        assert!(claim_url(&STANDARD.encode("http://insecure")).is_err());
        assert!(claim_url(&STANDARD.encode("http://bridge.simplefin.org/claim/abc")).is_err(), "not https");
        assert!(claim_url(&STANDARD.encode("https://bridge.example/claim/abc")).is_err(), "not the bridge");
        assert!(claim_url(&STANDARD.encode("https://bridge.simplefin.org.evil.example/claim")).is_err());
    }

    #[test]
    fn splits_access_url() {
        let (base, u, p) = split_access_url("https://user:secret@bridge.simplefin.org/simplefin").unwrap();
        assert_eq!(base, "https://bridge.simplefin.org/simplefin");
        assert_eq!((u.as_str(), p.as_str()), ("user", "secret"));
        let (_, _, p) = split_access_url("https://user:p%40ss%3Aword@beta-bridge.simplefin.org/simplefin").unwrap();
        assert_eq!(p, "p@ss:word", "percent-encoded passwords decode");
    }

    #[test]
    fn refuses_access_urls_that_leave_the_bridge() {
        for url in [
            "http://u:p@bridge.simplefin.org/simplefin",
            "https://u:p@ss@evil.example/x",
            "https://u:p@evil.example/simplefin",
            "https://bridge.simplefin.org/simplefin",
            "https://u@bridge.simplefin.org/simplefin",
            "not a url",
        ] {
            assert!(split_access_url(url).is_err(), "{url}");
        }
    }

    #[test]
    fn parses_account_set() {
        let body = r#"{"errors": [], "accounts": [
            {"org": {"name": "Credit Union"}, "id": "acct-1", "name": "Checking", "currency": "USD",
             "balance": "1520.03", "balance-date": 1767225600,
             "transactions": [
                {"id": "t-1", "posted": 1767225600, "amount": "-33.29", "description": "GROCERY", "payee": "Safeway"},
                {"id": "t-2", "posted": 0, "transacted_at": 1767312000, "amount": "200.00", "description": "VENMO", "pending": true}
             ]},
            {"id": "acct-2", "name": "Visa", "balance": "-250.00", "transactions": []}
        ]}"#;
        let accts = parse_account_set(body).unwrap();
        assert_eq!(accts[0].institution.as_deref(), Some("Credit Union"));
        assert_eq!(accts[0].transactions[0].amount, Cents(-3329));
        assert_eq!(accts[0].transactions[0].date, NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
        assert!(accts[0].transactions[1].pending);
        assert_eq!(accts[1].account.kind, FeedAccountKind::Credit);
        assert_eq!(accts[1].account.current_balance, Some(Cents(25000)));
    }

    #[test]
    fn surfaces_bridge_errors() {
        let e = parse_account_set(r#"{"errors": ["You must reauthenticate."], "accounts": []}"#).unwrap_err();
        assert!(e.to_string().contains("reauthenticate"));
    }
}
