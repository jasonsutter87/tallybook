//! Bank-feed adapters. Each turns a provider's format into
//! [`tally_core::bank::FeedTransaction`]s (inflow-positive amounts).
//!
//! Nothing here touches the books; the app layer hands the results to
//! `tally-store`.

pub mod csv_import;
pub mod ofx;
pub mod plaid;
pub mod simplefin;

use tally_core::money::Cents;

/// Converts a JSON float amount (Plaid) to cents without float drift.
pub(crate) fn float_to_cents(x: f64) -> Option<Cents> {
    if !x.is_finite() || x.abs() > 9.0e15 {
        return None;
    }
    Some(Cents((x * 100.0).round() as i64))
}
