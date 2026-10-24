//! License checks against Lemon Squeezy's License API, with a small cache.

use crate::AppState;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const VALID_TTL: Duration = Duration::from_secs(60 * 60);
const INVALID_TTL: Duration = Duration::from_secs(5 * 60);
const LS_TIMEOUT: Duration = Duration::from_secs(10);
const CACHE_PRUNE_ABOVE: usize = 10_000;

/// What Lemon Squeezy says about a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Active, ours, and on a product that includes banks.
    Valid,
    /// Ours, but expired or disabled: the plan has ended.
    Lapsed,
    /// Unknown key, another store's, or a product without banks.
    Rejected,
    /// Network error or a reply we don't understand. Never cached, never
    /// grounds for removing anything.
    Unavailable,
}

pub fn sha256_hex(s: &str) -> String {
    Sha256::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Default)]
pub struct Cache {
    entries: Mutex<HashMap<String, (Verdict, Instant)>>,
}

impl Cache {
    fn get(&self, hash: &str) -> Option<Verdict> {
        let map = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let (verdict, at) = map.get(hash)?;
        let ttl = if *verdict == Verdict::Valid { VALID_TTL } else { INVALID_TTL };
        (at.elapsed() < ttl).then_some(*verdict)
    }

    fn put(&self, hash: &str, verdict: Verdict) {
        let mut map = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        if map.len() > CACHE_PRUNE_ABOVE {
            map.retain(|_, (v, at)| at.elapsed() < if *v == Verdict::Valid { VALID_TTL } else { INVALID_TTL });
        }
        map.insert(hash.to_string(), (verdict, Instant::now()));
    }
}

/// Lemon Squeezy sends ids as numbers; accept strings too.
fn id_of(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

impl AppState {
    /// Asks Lemon Squeezy, bypassing the cache.
    pub async fn check_license(&self, key: &str) -> Verdict {
        let url = format!("{}/v1/licenses/validate", self.cfg.ls_base);
        let resp = self.http.post(url).timeout(LS_TIMEOUT).header("Accept", "application/json").form(&[("license_key", key)]).send().await;
        let Ok(resp) = resp else { return Verdict::Unavailable };
        let http_status = resp.status();
        if http_status.is_server_error() || http_status.as_u16() == 429 || http_status.as_u16() == 401 || http_status.as_u16() == 403 {
            return Verdict::Unavailable;
        }
        let Ok(body) = resp.json::<Value>().await else { return Verdict::Unavailable };
        let Some(valid) = body.get("valid").and_then(Value::as_bool) else { return Verdict::Unavailable };

        let ours = id_of(body.pointer("/meta/store_id")).is_some_and(|s| s == self.cfg.ls_store_id)
            && id_of(body.pointer("/meta/product_id")).is_some_and(|p| self.cfg.ls_product_ids.contains(&p));
        if !ours {
            return Verdict::Rejected;
        }
        match (valid, body.pointer("/license_key/status").and_then(Value::as_str)) {
            (true, Some("active")) => Verdict::Valid,
            (_, Some("expired" | "disabled")) => Verdict::Lapsed,
            _ => Verdict::Rejected,
        }
    }

    /// The cached verdict for a request's key, asking Lemon Squeezy on a miss.
    /// A fresh `Valid` is recorded so the lapse sweeper knows the license.
    pub async fn verdict_for(&self, key: &str, hash: &str) -> Verdict {
        if let Some(v) = self.cache.get(hash) {
            return v;
        }
        let verdict = self.check_license(key).await;
        match verdict {
            Verdict::Unavailable => {}
            Verdict::Valid => {
                self.cache.put(hash, verdict);
                if let Err(e) = self.db.license_valid(hash, key, chrono::Utc::now().timestamp()) {
                    (self.log)(&format!("db error recording license: {e}"));
                }
            }
            _ => self.cache.put(hash, verdict),
        }
        verdict
    }
}
