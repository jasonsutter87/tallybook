//! Configuration, from environment variables only.

use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{0} is required")]
    Missing(&'static str),
    #[error("{0} is invalid: {1}")]
    Invalid(&'static str, String),
}

/// Everything the service needs. No `Debug` on purpose: it holds secrets.
#[derive(Clone)]
pub struct Config {
    pub db_path: PathBuf,
    pub db_key: String,
    pub plaid_client_id: String,
    pub plaid_secret: String,
    /// `https://sandbox.plaid.com` or `https://production.plaid.com` (or a test fake).
    pub plaid_base: String,
    pub ls_store_id: String,
    pub ls_product_ids: HashSet<String>,
    /// `https://api.lemonsqueezy.com` unless a test overrides it.
    pub ls_base: String,
    pub bind: String,
    pub max_items_per_license: u32,
    pub lapse_grace_days: i64,
    pub rate_limit_per_min: u32,
    /// Header carrying the real client address when behind a proxy that sets
    /// it (`CF-Connecting-IP`, or `X-Forwarded-For`, whose last entry is used).
    /// Unset: the socket's peer address.
    pub client_ip_header: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Config, ConfigError> {
        Config::from_lookup(|k| std::env::var(k).ok())
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Config, ConfigError> {
        let opt = |k: &str| get(k).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        let req = |k: &'static str| opt(k).ok_or(ConfigError::Missing(k));
        let num = |k: &'static str, default: i64| -> Result<i64, ConfigError> {
            match opt(k) {
                None => Ok(default),
                Some(v) => v.parse::<i64>().ok().filter(|n| *n >= 0).ok_or_else(|| ConfigError::Invalid(k, "expected a non-negative whole number".into())),
            }
        };

        let plaid_base = match opt("PLAID_BASE_URL") {
            Some(url) => url,
            None => match req("PLAID_ENV")?.as_str() {
                "sandbox" => "https://sandbox.plaid.com".to_string(),
                "production" => "https://production.plaid.com".to_string(),
                other => return Err(ConfigError::Invalid("PLAID_ENV", format!("expected sandbox or production, got {other:?}"))),
            },
        };
        let ls_product_ids: HashSet<String> = req("LS_PRODUCT_IDS")?.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
        if ls_product_ids.is_empty() {
            return Err(ConfigError::Invalid("LS_PRODUCT_IDS", "no product ids".into()));
        }
        Ok(Config {
            db_path: PathBuf::from(req("CONNECT_DB")?),
            db_key: req("CONNECT_DB_KEY")?,
            plaid_client_id: req("PLAID_CLIENT_ID")?,
            plaid_secret: req("PLAID_SECRET")?,
            plaid_base: plaid_base.trim_end_matches('/').to_string(),
            ls_store_id: req("LS_STORE_ID")?,
            ls_product_ids,
            ls_base: opt("LS_BASE_URL").unwrap_or_else(|| "https://api.lemonsqueezy.com".into()).trim_end_matches('/').to_string(),
            bind: opt("BIND").unwrap_or_else(|| "127.0.0.1:8787".into()),
            max_items_per_license: num("MAX_ITEMS_PER_LICENSE", 10)? as u32,
            lapse_grace_days: num("LAPSE_GRACE_DAYS", 30)?,
            rate_limit_per_min: num("RATE_LIMIT_PER_MIN", 120)? as u32,
            client_ip_header: opt("CLIENT_IP_HEADER"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(extra: &[(&str, &str)], skip: &[&str]) -> HashMap<String, String> {
        let mut m: HashMap<String, String> = [
            ("CONNECT_DB", "/tmp/x.db"),
            ("CONNECT_DB_KEY", "k"),
            ("PLAID_CLIENT_ID", "id"),
            ("PLAID_SECRET", "s"),
            ("PLAID_ENV", "sandbox"),
            ("LS_STORE_ID", "1"),
            ("LS_PRODUCT_IDS", "2, 3"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        for (k, v) in extra {
            m.insert(k.to_string(), v.to_string());
        }
        for k in skip {
            m.remove(*k);
        }
        m
    }

    #[test]
    fn defaults_and_product_list() {
        let m = env(&[], &[]);
        let c = Config::from_lookup(|k| m.get(k).cloned()).unwrap();
        assert_eq!(c.bind, "127.0.0.1:8787");
        assert_eq!(c.max_items_per_license, 10);
        assert_eq!(c.lapse_grace_days, 30);
        assert_eq!(c.plaid_base, "https://sandbox.plaid.com");
        assert!(c.ls_product_ids.contains("2") && c.ls_product_ids.contains("3"));
    }

    #[test]
    fn refuses_to_start_without_required_settings() {
        for skip in ["CONNECT_DB_KEY", "PLAID_SECRET", "LS_STORE_ID"] {
            let m = env(&[], &[skip]);
            assert!(Config::from_lookup(|k| m.get(k).cloned()).is_err(), "{skip}");
        }
        let m = env(&[("CONNECT_DB_KEY", "  ")], &[]);
        assert!(Config::from_lookup(|k| m.get(k).cloned()).is_err());
        let m = env(&[("PLAID_ENV", "development")], &[]);
        assert!(Config::from_lookup(|k| m.get(k).cloned()).is_err());
    }
}
