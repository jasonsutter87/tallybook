//! Chart-of-accounts types.

use crate::money::Cents;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

pub type AccountId = i64;

/// The five fundamental account classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountKind {
    Asset,
    Liability,
    Equity,
    Income,
    Expense,
}

impl AccountKind {
    /// Assets and expenses increase with debits; everything else with credits.
    pub fn is_debit_normal(self) -> bool {
        matches!(self, AccountKind::Asset | AccountKind::Expense)
    }

    pub fn is_balance_sheet(self) -> bool {
        matches!(self, AccountKind::Asset | AccountKind::Liability | AccountKind::Equity)
    }

    /// Converts a raw debit-positive balance to this kind's natural sign.
    pub fn natural(self, raw: Cents) -> Cents {
        if self.is_debit_normal() { raw } else { -raw }
    }

    /// Inverse of [`AccountKind::natural`].
    pub fn raw(self, natural: Cents) -> Cents {
        self.natural(natural)
    }
}

/// Finer-grained type that drives report grouping and UI behavior
/// (e.g. only `Bank` and `CreditCard` accounts can link to a bank feed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountSubtype {
    Bank,
    AccountsReceivable,
    OtherCurrentAsset,
    FixedAsset,
    CreditCard,
    AccountsPayable,
    OtherCurrentLiability,
    LongTermLiability,
    Equity,
    RetainedEarnings,
    Income,
    OtherIncome,
    CostOfGoodsSold,
    Expense,
    OtherExpense,
}

impl AccountSubtype {
    pub const ALL: [AccountSubtype; 15] = [
        AccountSubtype::Bank,
        AccountSubtype::AccountsReceivable,
        AccountSubtype::OtherCurrentAsset,
        AccountSubtype::FixedAsset,
        AccountSubtype::CreditCard,
        AccountSubtype::AccountsPayable,
        AccountSubtype::OtherCurrentLiability,
        AccountSubtype::LongTermLiability,
        AccountSubtype::Equity,
        AccountSubtype::RetainedEarnings,
        AccountSubtype::Income,
        AccountSubtype::OtherIncome,
        AccountSubtype::CostOfGoodsSold,
        AccountSubtype::Expense,
        AccountSubtype::OtherExpense,
    ];

    pub fn kind(self) -> AccountKind {
        use AccountSubtype::*;
        match self {
            Bank | AccountsReceivable | OtherCurrentAsset | FixedAsset => AccountKind::Asset,
            CreditCard | AccountsPayable | OtherCurrentLiability | LongTermLiability => AccountKind::Liability,
            Equity | RetainedEarnings => AccountKind::Equity,
            Income | OtherIncome => AccountKind::Income,
            CostOfGoodsSold | Expense | OtherExpense => AccountKind::Expense,
        }
    }

    /// Accounts whose register mirrors a real-world statement.
    pub fn is_reconcilable(self) -> bool {
        matches!(
            self,
            AccountSubtype::Bank
                | AccountSubtype::CreditCard
                | AccountSubtype::OtherCurrentAsset
                | AccountSubtype::OtherCurrentLiability
                | AccountSubtype::LongTermLiability
        )
    }

    pub fn as_str(self) -> &'static str {
        use AccountSubtype::*;
        match self {
            Bank => "bank",
            AccountsReceivable => "accounts_receivable",
            OtherCurrentAsset => "other_current_asset",
            FixedAsset => "fixed_asset",
            CreditCard => "credit_card",
            AccountsPayable => "accounts_payable",
            OtherCurrentLiability => "other_current_liability",
            LongTermLiability => "long_term_liability",
            Equity => "equity",
            RetainedEarnings => "retained_earnings",
            Income => "income",
            OtherIncome => "other_income",
            CostOfGoodsSold => "cost_of_goods_sold",
            Expense => "expense",
            OtherExpense => "other_expense",
        }
    }
}

impl fmt::Display for AccountSubtype {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown account subtype: {0:?}")]
pub struct UnknownSubtype(pub String);

impl FromStr for AccountSubtype {
    type Err = UnknownSubtype;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        AccountSubtype::ALL
            .into_iter()
            .find(|t| t.as_str() == s)
            .ok_or_else(|| UnknownSubtype(s.to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub id: AccountId,
    pub code: Option<String>,
    pub name: String,
    pub subtype: AccountSubtype,
    pub parent_id: Option<AccountId>,
    pub archived: bool,
}

impl Account {
    pub fn kind(&self) -> AccountKind {
        self.subtype.kind()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtype_round_trips() {
        for t in AccountSubtype::ALL {
            assert_eq!(t.as_str().parse::<AccountSubtype>().unwrap(), t);
        }
        assert!("nope".parse::<AccountSubtype>().is_err());
    }

    #[test]
    fn natural_sign() {
        assert_eq!(AccountKind::Asset.natural(Cents(500)), Cents(500));
        assert_eq!(AccountKind::Income.natural(Cents(-500)), Cents(500));
        assert_eq!(AccountKind::Liability.raw(Cents(500)), Cents(-500));
    }
}
