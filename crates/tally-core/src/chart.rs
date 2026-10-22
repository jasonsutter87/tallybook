//! Starter charts of accounts: one for a small US business, one for a
//! household. Household codes start with `H` so both fit in one book.
//!
//! The two sides meet through a bridge. When one payment mixes business and
//! household money (the business card buys groceries), the store adds
//! balancing lines so each side still sums to zero: an owner's draw on the
//! business (`3100`) paired with "From the Business" at home (`H4800`), or
//! for money going the other way, owner's equity (`3000`) paired with
//! "Into the Business" (`H6800`).

use crate::account::{AccountSubtype, Flex, Scope};

/// One account a new book starts with.
pub struct ChartTemplate {
    pub code: &'static str,
    pub name: &'static str,
    pub subtype: AccountSubtype,
    /// Set on household spending categories; `None` elsewhere.
    pub flex: Option<Flex>,
}

/// A template with no flex: business accounts and non-spending household ones.
const fn t(code: &'static str, name: &'static str, subtype: AccountSubtype) -> ChartTemplate {
    ChartTemplate { code, name, subtype, flex: None }
}

/// A household spending category and how far it bends.
const fn h(code: &'static str, name: &'static str, flex: Flex) -> ChartTemplate {
    ChartTemplate { code, name, subtype: AccountSubtype::Expense, flex: Some(flex) }
}

/// The starter chart for one side of a book.
pub fn chart_for(scope: Scope) -> &'static [ChartTemplate] {
    match scope {
        Scope::Business => DEFAULT_CHART,
        Scope::Household => HOUSEHOLD_CHART,
    }
}

/// Codes that other parts of the app look up by convention. Changing one
/// orphans the account in every existing book, so these are permanent.
pub mod codes {
    pub const ACCOUNTS_RECEIVABLE: &str = "1100";
    pub const UNDEPOSITED_FUNDS: &str = "1200";
    pub const ACCOUNTS_PAYABLE: &str = "2100";
    pub const SALES_TAX_PAYABLE: &str = "2200";
    pub const OPENING_BALANCE_EQUITY: &str = "3050";
    pub const RETAINED_EARNINGS: &str = "3900";
    pub const UNCATEGORIZED_INCOME: &str = "4999";
    pub const UNCATEGORIZED_EXPENSE: &str = "6999";
    pub const OWNERS_EQUITY: &str = "3000";
    pub const OWNERS_DRAW: &str = "3100";

    pub const HOUSEHOLD_OPENING_BALANCE: &str = "H3050";
    pub const HOUSEHOLD_UNCATEGORIZED_INCOME: &str = "H4999";
    pub const HOUSEHOLD_UNCATEGORIZED_EXPENSE: &str = "H6999";
    /// Household side of an owner's draw: money that came home from the business.
    pub const FROM_THE_BUSINESS: &str = "H4800";
    /// Household side of an owner's contribution: money put into the business.
    pub const INTO_THE_BUSINESS: &str = "H6800";
}

/// Opening Balance Equity for a side of the book.
pub fn opening_balance_code(scope: Scope) -> &'static str {
    match scope {
        Scope::Business => codes::OPENING_BALANCE_EQUITY,
        Scope::Household => codes::HOUSEHOLD_OPENING_BALANCE,
    }
}

/// The catch-all category for a side of the book.
pub fn uncategorized_expense_code(scope: Scope) -> &'static str {
    match scope {
        Scope::Business => codes::UNCATEGORIZED_EXPENSE,
        Scope::Household => codes::HOUSEHOLD_UNCATEGORIZED_EXPENSE,
    }
}

/// The business starter chart. Codes follow the common US small-business
/// numbering: 1xxx assets, 2xxx liabilities, 3xxx equity, 4xxx income,
/// 5xxx cost of goods, 6xxx-7xxx expenses.
pub const DEFAULT_CHART: &[ChartTemplate] = {
    use AccountSubtype::*;
    &[
        t("1100", "Accounts Receivable", AccountsReceivable),
        t("1200", "Undeposited Funds", OtherCurrentAsset),
        t("1500", "Equipment", FixedAsset),
        t("2100", "Accounts Payable", AccountsPayable),
        t("2200", "Sales Tax Payable", OtherCurrentLiability),
        t("3000", "Owner's Equity", Equity),
        t("3050", "Opening Balance Equity", Equity),
        t("3100", "Owner's Draw", Equity),
        t("3900", "Retained Earnings", RetainedEarnings),
        t("4000", "Sales", Income),
        t("4100", "Service Revenue", Income),
        t("4900", "Interest Income", OtherIncome),
        t("4999", "Uncategorized Income", Income),
        t("5000", "Cost of Goods Sold", CostOfGoodsSold),
        t("6000", "Advertising & Marketing", Expense),
        t("6100", "Bank Fees", Expense),
        t("6150", "Contractors", Expense),
        t("6200", "Insurance", Expense),
        t("6250", "Meals", Expense),
        t("6300", "Office Supplies", Expense),
        t("6350", "Professional Fees", Expense),
        t("6400", "Rent", Expense),
        t("6500", "Software & Subscriptions", Expense),
        t("6600", "Travel", Expense),
        t("6700", "Utilities", Expense),
        t("6999", "Uncategorized Expense", Expense),
        t("7000", "Interest Expense", OtherExpense),
    ]
};

/// Drawn from real household budgets: the categories a family actually
/// tracks, each with how far it can bend. Bank and card accounts are added by
/// the user, as in the business chart. Same numbering as the business chart,
/// prefixed with `H`.
pub const HOUSEHOLD_CHART: &[ChartTemplate] = {
    use AccountSubtype::*;
    use Flex::*;
    &[
        t("H3050", "Opening Balances", Equity),
        t("H4000", "Paychecks", Income),
        t("H4100", "Child Support", Income),
        t("H4200", "Side Income", Income),
        t("H4300", "Gifts Received", Income),
        t("H4400", "Refunds & Reimbursements", Income),
        t("H4800", "From the Business", Income),
        t("H4900", "Interest & Dividends", OtherIncome),
        t("H4999", "Uncategorized Income", Income),
        h("H6000", "Rent or Mortgage", Fixed),
        h("H6010", "Renters or Home Insurance", Fixed),
        h("H6020", "Home Repairs & Supplies", Lifestyle),
        h("H6030", "Furnishings & Home Goods", Lifestyle),
        h("H6100", "Gas & Electric", Usage),
        h("H6110", "Water, Sewer & Garbage", Usage),
        h("H6120", "Internet & TV", Fixed),
        h("H6130", "Phones", Fixed),
        h("H6200", "Groceries", Usage),
        h("H6210", "Dining Out", Lifestyle),
        h("H6220", "Coffee & Treats", Lifestyle),
        h("H6300", "Car Payment", Fixed),
        h("H6310", "Car Insurance", Fixed),
        h("H6320", "Gas & Fuel", Usage),
        h("H6330", "Car Maintenance & Registration", Usage),
        h("H6400", "Health Insurance", Fixed),
        h("H6410", "Medical & Dental", Usage),
        h("H6420", "Fitness & Wellness", Lifestyle),
        h("H6430", "Life & Disability Insurance", Fixed),
        h("H6500", "Childcare & School", Fixed),
        h("H6510", "Kids' Activities & Sports", Lifestyle),
        h("H6600", "Pet Supplies", Usage),
        h("H6610", "Vet", Usage),
        h("H6700", "Clothing", Lifestyle),
        h("H6710", "Personal Care", Lifestyle),
        h("H6720", "Shopping", Lifestyle),
        h("H6730", "Subscriptions & Streaming", Lifestyle),
        h("H6740", "Fun & Entertainment", Lifestyle),
        h("H6750", "Gifts & Holidays", Lifestyle),
        h("H6760", "Travel", Lifestyle),
        h("H6770", "Giving & Tithing", Lifestyle),
        h("H6780", "Taxes & Tax Prep", Fixed),
        h("H6790", "Loan & Card Interest", Fixed),
        t("H6800", "Into the Business", Expense),
        t("H6999", "Uncategorized Spending", Expense),
    ]
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn charts_can_share_a_book() {
        let mut seen = HashSet::new();
        for t in DEFAULT_CHART.iter().chain(HOUSEHOLD_CHART) {
            assert!(seen.insert(t.code), "duplicate code {}", t.code);
        }
        assert!(HOUSEHOLD_CHART.iter().all(|t| t.code.starts_with('H')));
    }

    #[test]
    fn conventional_codes_exist() {
        let has = |chart: &[ChartTemplate], code: &str| chart.iter().any(|t| t.code == code);
        for scope in Scope::ALL {
            assert!(has(chart_for(scope), opening_balance_code(scope)));
            assert!(has(chart_for(scope), uncategorized_expense_code(scope)));
        }
        for c in [codes::OWNERS_EQUITY, codes::OWNERS_DRAW] {
            assert!(has(DEFAULT_CHART, c));
        }
        for c in [codes::FROM_THE_BUSINESS, codes::INTO_THE_BUSINESS, codes::HOUSEHOLD_UNCATEGORIZED_INCOME] {
            assert!(has(HOUSEHOLD_CHART, c));
        }
    }

    #[test]
    fn spending_categories_say_how_far_they_bend() {
        // The bridge and the catch-all aren't choices anyone can bend.
        let not_categories = [codes::INTO_THE_BUSINESS, codes::HOUSEHOLD_UNCATEGORIZED_EXPENSE];
        for t in HOUSEHOLD_CHART.iter().filter(|t| t.subtype == AccountSubtype::Expense && !not_categories.contains(&t.code)) {
            assert!(t.flex.is_some(), "{} has no flex", t.name);
        }
    }
}
