//! Provider-neutral bank-feed records.
//!
//! Every feed (Plaid, SimpleFIN, CSV, OFX) is normalized into these before it
//! touches the books. `amount` is **inflow-positive from the account's point
//! of view**: a deposit or a credit-card payment is positive, a purchase or
//! withdrawal negative. That makes it identical to the raw (debit-positive)
//! amount of the bank/card account's journal line, for both assets and
//! liabilities.

use crate::money::Cents;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedTransaction {
    /// Stable id from the provider, unique within one bank account.
    pub external_id: String,
    pub date: NaiveDate,
    pub amount: Cents,
    pub description: String,
    pub merchant: Option<String>,
    pub pending: bool,
    /// Provider's own category, used only as a hint.
    pub category_hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedAccountKind {
    Depository,
    Credit,
    Loan,
    Investment,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedAccount {
    pub external_id: String,
    pub name: String,
    pub mask: Option<String>,
    pub kind: FeedAccountKind,
    /// Natural sign: what the bank says you have (or owe, for credit).
    pub current_balance: Option<Cents>,
    pub currency: Option<String>,
}

/// One page (or the whole) of changes from a feed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedBatch {
    pub added: Vec<FeedTransaction>,
    pub modified: Vec<FeedTransaction>,
    pub removed: Vec<String>,
}

/// Lowercased, digit-free, whitespace-collapsed description used to recognize
/// the same payee across months ("SQ *BLUE BOTTLE 0423" ≈ "SQ *BLUE BOTTLE 0517").
pub fn normalize_description(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for word in s.split_whitespace() {
        let w: String = word
            .chars()
            .filter(|c| !c.is_ascii_digit() && *c != '#')
            .flat_map(char::to_lowercase)
            .collect();
        if w.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&w);
    }
    out
}

/// Stable 64-bit FNV-1a, for synthesizing ids for feeds that don't provide them (CSV).
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_noisy_descriptions() {
        assert_eq!(normalize_description("SQ *BLUE BOTTLE 0423"), "sq *blue bottle");
        assert_eq!(normalize_description("  Amazon.com   #1234 "), "amazon.com");
    }

    #[test]
    fn fnv_is_stable() {
        assert_eq!(fnv1a64(b""), 0xcbf29ce484222325);
        assert_eq!(fnv1a64(b"a"), 0xaf63dc4c8601ec8c);
    }
}
