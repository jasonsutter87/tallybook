//! Tallybook Connect: lets paying users link banks through Plaid without
//! bringing their own Plaid keys. See `docs/CONNECT.md`.
//!
//! The service is a thin, stateless-by-design proxy. It holds our Plaid keys
//! and the real Plaid access tokens; clients only ever see opaque handles. It
//! stores no books and no transactions.

pub mod config;
pub mod db;
pub mod license;
pub mod limit;
pub mod proxy;
pub mod sweep;

use config::Config;
use std::sync::Arc;
use std::time::Duration;

/// Where log lines go. Lines never contain keys, tokens, handles or bodies.
pub type LogSink = Arc<dyn Fn(&str) + Send + Sync>;

/// Failed authentications allowed per client address per minute.
const FAILED_AUTH_PER_MIN: u32 = 20;
/// Timeouts for calls out to Plaid; `/transactions/sync` pages can be slow.
const PLAID_TIMEOUT: Duration = Duration::from_secs(60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

pub struct AppState {
    pub cfg: Config,
    pub db: db::Db,
    pub http: reqwest::Client,
    pub log: LogSink,
    pub cache: license::Cache,
    pub limiter: limit::Limiter,
    pub auth_fail: limit::Limiter,
}

impl AppState {
    pub fn new(cfg: Config, log: LogSink) -> Result<Arc<AppState>, String> {
        let db = db::Db::open(&cfg.db_path, &cfg.db_key).map_err(|e| e.to_string())?;
        let http = reqwest::Client::builder().timeout(PLAID_TIMEOUT).connect_timeout(CONNECT_TIMEOUT).build().map_err(|e| format!("http client: {e}"))?;
        Ok(Arc::new(AppState {
            limiter: limit::Limiter::new(cfg.rate_limit_per_min, Duration::from_secs(60)),
            auth_fail: limit::Limiter::new(FAILED_AUTH_PER_MIN, Duration::from_secs(60)),
            cache: license::Cache::default(),
            cfg,
            db,
            http,
            log,
        }))
    }
}

/// A log sink that writes to standard error.
pub fn stderr_log() -> LogSink {
    Arc::new(|line| eprintln!("{} {line}", chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ")))
}
