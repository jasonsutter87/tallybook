//! Bank-feed categorization rules ("anything containing GUSTO → Payroll").

use crate::account::AccountId;
use crate::money::Cents;
use serde::{Deserialize, Serialize};

pub type RuleId = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchOp {
    Contains,
    Equals,
    StartsWith,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Any,
    Inflow,
    Outflow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub id: RuleId,
    pub name: String,
    /// Lower runs first.
    pub priority: i64,
    pub op: MatchOp,
    /// Compared case-insensitively against the description.
    pub pattern: String,
    pub direction: Direction,
    /// Bounds on the absolute amount, inclusive.
    pub amount_min: Option<Cents>,
    pub amount_max: Option<Cents>,
    /// Restrict to one bank/card account; `None` = all.
    pub bank_account_id: Option<AccountId>,
    pub target_account_id: AccountId,
    pub rename_to: Option<String>,
    /// Post without waiting for review.
    pub auto_post: bool,
    /// Who the money is for (household books), e.g. `Roblox` → a kid.
    #[serde(default)]
    pub person_id: Option<i64>,
}

impl Rule {
    pub fn matches(&self, bank_account_id: AccountId, description: &str, amount: Cents) -> bool {
        if self.bank_account_id.is_some_and(|id| id != bank_account_id) {
            return false;
        }
        match self.direction {
            Direction::Inflow if amount.0 <= 0 => return false,
            Direction::Outflow if amount.0 >= 0 => return false,
            _ => {}
        }
        let abs = amount.abs();
        if self.amount_min.is_some_and(|m| abs < m) || self.amount_max.is_some_and(|m| abs > m) {
            return false;
        }
        let hay = description.to_lowercase();
        let needle = self.pattern.trim().to_lowercase();
        if needle.is_empty() {
            return false;
        }
        match self.op {
            MatchOp::Contains => hay.contains(&needle),
            MatchOp::Equals => hay.trim() == needle,
            MatchOp::StartsWith => hay.trim_start().starts_with(&needle),
        }
    }
}

/// First matching rule by (priority, id).
pub fn first_match<'a>(rules: &'a [Rule], bank_account_id: AccountId, description: &str, amount: Cents) -> Option<&'a Rule> {
    let mut sorted: Vec<&Rule> = rules.iter().collect();
    sorted.sort_by_key(|r| (r.priority, r.id));
    sorted.into_iter().find(|r| r.matches(bank_account_id, description, amount))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(id: RuleId, priority: i64, pattern: &str) -> Rule {
        Rule {
            id,
            name: pattern.into(),
            priority,
            op: MatchOp::Contains,
            pattern: pattern.into(),
            direction: Direction::Any,
            amount_min: None,
            amount_max: None,
            bank_account_id: None,
            target_account_id: 100 + id,
            rename_to: None,
            auto_post: false,
            person_id: None,
        }
    }

    #[test]
    fn matches_case_insensitively_by_priority() {
        let rules = vec![rule(1, 10, "amazon"), rule(2, 5, "AMAZON WEB")];
        let hit = first_match(&rules, 1, "Amazon Web Services", Cents(-100)).unwrap();
        assert_eq!(hit.id, 2);
        assert_eq!(first_match(&rules, 1, "amazon.com", Cents(-100)).unwrap().id, 1);
        assert!(first_match(&rules, 1, "target", Cents(-100)).is_none());
    }

    #[test]
    fn honors_direction_amount_and_account() {
        let mut r = rule(1, 0, "stripe");
        r.direction = Direction::Inflow;
        assert!(r.matches(1, "STRIPE TRANSFER", Cents(500)));
        assert!(!r.matches(1, "STRIPE FEE", Cents(-500)));

        r.amount_min = Some(Cents(100));
        r.amount_max = Some(Cents(1000));
        assert!(!r.matches(1, "stripe", Cents(50)));
        assert!(!r.matches(1, "stripe", Cents(5000)));

        r.bank_account_id = Some(7);
        assert!(!r.matches(1, "stripe", Cents(500)));
        assert!(r.matches(7, "stripe", Cents(500)));
    }

    #[test]
    fn empty_pattern_never_matches() {
        assert!(!rule(1, 0, "  ").matches(1, "anything", Cents(1)));
    }
}
