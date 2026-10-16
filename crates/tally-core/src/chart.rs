//! Starter chart of accounts for a small US business.

use crate::account::AccountSubtype;

pub struct ChartTemplate {
    pub code: &'static str,
    pub name: &'static str,
    pub subtype: AccountSubtype,
}

const fn t(code: &'static str, name: &'static str, subtype: AccountSubtype) -> ChartTemplate {
    ChartTemplate { code, name, subtype }
}

/// Codes that other parts of the app look up by convention.
pub mod codes {
    pub const ACCOUNTS_RECEIVABLE: &str = "1100";
    pub const UNDEPOSITED_FUNDS: &str = "1200";
    pub const ACCOUNTS_PAYABLE: &str = "2100";
    pub const SALES_TAX_PAYABLE: &str = "2200";
    pub const OPENING_BALANCE_EQUITY: &str = "3050";
    pub const RETAINED_EARNINGS: &str = "3900";
    pub const UNCATEGORIZED_INCOME: &str = "4999";
    pub const UNCATEGORIZED_EXPENSE: &str = "6999";
}

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
