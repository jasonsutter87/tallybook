//! The lapse sweeper.
//!
//! Plaid bills per Item per month until the Item is removed, so when a plan
//! has really ended we remove its Items at Plaid and forget them. The rule is
//! deliberately one-directional: only a clear "expired" or "disabled" from
//! Lemon Squeezy, sustained past the grace period, removes anything. A
//! network error, a rate limit, or a reply we don't understand skips.

use crate::AppState;
use crate::license::Verdict;
use crate::proxy::item_gone;
use serde_json::json;
use std::time::Duration;

const SECONDS_PER_DAY: i64 = 86_400;
const SWEEP_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// Delay before the first sweep after start-up, so restarts don't skip it
/// forever but a flapping service doesn't hammer Lemon Squeezy.
const FIRST_SWEEP_AFTER: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Default, PartialEq, Eq)]
pub struct SweepReport {
    pub licenses_checked: usize,
    pub licenses_in_grace: usize,
    pub items_removed: usize,
    pub skipped: usize,
}

/// Re-validates every license that has items. `now` is a unix timestamp.
pub async fn sweep_once(st: &AppState, now: i64) -> SweepReport {
    let mut report = SweepReport::default();
    let licenses = match st.db.licenses_with_items() {
        Ok(l) => l,
        Err(e) => {
            (st.log)(&format!("sweep: cannot list licenses: {e}"));
            return report;
        }
    };
    for lic in licenses {
        report.licenses_checked += 1;
        match st.check_license(&lic.license_key).await {
            Verdict::Valid => {
                let _ = st.db.license_valid(&lic.key_hash, &lic.license_key, now);
            }
            Verdict::Lapsed => {
                let _ = st.db.license_lapsed(&lic.key_hash, now);
                let since = lic.invalid_since.unwrap_or(now);
                if now - since <= st.cfg.lapse_grace_days * SECONDS_PER_DAY {
                    report.licenses_in_grace += 1;
                    continue;
                }
                report.items_removed += remove_items(st, &lic.key_hash).await;
                if st.db.count_items(&lic.key_hash).unwrap_or(1) == 0 {
                    let _ = st.db.delete_license(&lic.key_hash);
                }
            }
            Verdict::Rejected | Verdict::Unavailable => report.skipped += 1,
        }
    }
    (st.log)(&format!(
        "sweep: checked={} in_grace={} removed_items={} skipped={}",
        report.licenses_checked, report.licenses_in_grace, report.items_removed, report.skipped
    ));
    report
}

/// Removes a lapsed license's Items at Plaid, and locally only once Plaid
/// confirms (or says the Item is already gone). Returns how many.
async fn remove_items(st: &AppState, license_hash: &str) -> usize {
    let Ok(items) = st.db.items_of(license_hash) else { return 0 };
    let mut removed = 0;
    for item in items {
        match st.plaid_post("item/remove", json!({ "access_token": item.access_token })).await {
            Ok((status, v)) if status.is_success() || item_gone(&v) => {
                if st.db.delete_item(&item.handle).is_ok() {
                    removed += 1;
                }
            }
            _ => {} // try again tomorrow
        }
    }
    removed
}

/// Sweeps daily until the process exits.
pub async fn run_daily(st: std::sync::Arc<AppState>) {
    let mut ticker = tokio::time::interval_at(tokio::time::Instant::now() + FIRST_SWEEP_AFTER, SWEEP_EVERY);
    loop {
        ticker.tick().await;
        sweep_once(&st, chrono::Utc::now().timestamp()).await;
    }
}
