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
//!
//! A book may keep a business, a household, or both (see
//! [`account::Scope`]). The household side adds `budget` (time-aware plan
//! status), `schedule` (recurring bills and finding them in bank history)
//! and `insights` (goal projections and the budget score).

pub mod account;
pub mod bank;
pub mod budget;
pub mod chart;
pub mod insights;
pub mod invoice;
pub mod journal;
pub mod money;
pub mod reconcile;
pub mod report;
pub mod rules;
pub mod schedule;
pub mod tax;

pub use account::{Account, AccountId, AccountKind, AccountSubtype, Flex, Scope};
pub use journal::{EntryError, EntryLine, NewEntry};
pub use money::Cents;
