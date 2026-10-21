//! Tallybook's accounting core.
//!
//! Everything in this crate is pure: no database, no network, no clock.
//! The storage layer feeds it data and it answers accounting questions.
//!
//! Sign convention used everywhere below the UI: a journal line's `amount`
//! is **positive for a debit, negative for a credit**. Every entry's lines
//! sum to zero. Reports convert to each account's "natural" sign (assets
//! and expenses grow with debits; liabilities, equity and income grow with
//! credits) only at the edge.

pub mod account;
pub mod bank;
pub mod chart;
pub mod invoice;
pub mod journal;
pub mod money;
pub mod reconcile;
pub mod report;
pub mod rules;
pub mod tax;

pub use account::{Account, AccountId, AccountKind, AccountSubtype, Flex, Scope};
pub use journal::{EntryError, EntryLine, NewEntry};
pub use money::Cents;
