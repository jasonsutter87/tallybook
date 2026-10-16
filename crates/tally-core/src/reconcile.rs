//! Statement reconciliation arithmetic.

use crate::account::AccountKind;
use crate::money::Cents;
use serde::Serialize;

/// All amounts in the account's natural sign, so a credit card's statement
/// balance is "amount owed" just like on the paper statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ReconcileSummary {
    /// Balance of everything reconciled in earlier sessions.
    pub beginning_balance: Cents,
    /// Net of the lines checked off in this session.
    pub cleared_total: Cents,
    pub cleared_balance: Cents,
    pub statement_balance: Cents,
    /// Must be zero to finish.
    pub difference: Cents,
}

impl ReconcileSummary {
    pub fn compute(
        kind: AccountKind,
        previously_reconciled_raw: Cents,
        cleared_raw: impl IntoIterator<Item = Cents>,
        statement_balance: Cents,
    ) -> Self {
        let beginning_balance = kind.natural(previously_reconciled_raw);
        let cleared_total = kind.natural(cleared_raw.into_iter().sum());
        let cleared_balance = beginning_balance + cleared_total;
        ReconcileSummary {
            beginning_balance,
            cleared_total,
            cleared_balance,
            statement_balance,
            difference: statement_balance - cleared_balance,
        }
    }

    pub fn is_balanced(&self) -> bool {
        self.difference.is_zero()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bank_account() {
        // Began at 1000, deposit 500, check -200 → statement 1300.
        let s = ReconcileSummary::compute(AccountKind::Asset, Cents(1000), [Cents(500), Cents(-200)], Cents(1300));
        assert!(s.is_balanced());
        let off = ReconcileSummary::compute(AccountKind::Asset, Cents(1000), [Cents(500)], Cents(1300));
        assert_eq!(off.difference, Cents(-200));
    }

    #[test]
    fn credit_card_uses_amount_owed() {
        // Owed 100 (raw -100), new charge 40 (raw -40), payment 100 (raw +100) → owe 40.
        let s = ReconcileSummary::compute(AccountKind::Liability, Cents(-100), [Cents(-40), Cents(100)], Cents(40));
        assert!(s.is_balanced());
    }
}
