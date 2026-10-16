//! Financial statements computed from account balances.
//!
//! Inputs are raw (debit-positive) balances keyed by account; outputs are in
//! each account's natural sign, which is what humans expect to read.

use crate::account::{Account, AccountId, AccountKind, AccountSubtype};
use crate::money::Cents;
use serde::Serialize;
use std::collections::HashMap;

/// Raw debit-positive balance per account. Missing means zero.
pub type Balances = HashMap<AccountId, Cents>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReportLine {
    pub account_id: AccountId,
    pub code: Option<String>,
    pub name: String,
    pub subtype: AccountSubtype,
    /// Natural sign: a positive expense is money spent, a positive liability is money owed.
    pub amount: Cents,
}

fn raw(balances: &Balances, id: AccountId) -> Cents {
    balances.get(&id).copied().unwrap_or_default()
}

fn sorted(accounts: &[Account]) -> Vec<&Account> {
    let mut v: Vec<&Account> = accounts.iter().collect();
    v.sort_by(|a, b| {
        (a.code.is_none(), &a.code, &a.name).cmp(&(b.code.is_none(), &b.code, &b.name))
    });
    v
}

/// Non-zero accounts matching `keep`, natural sign, in chart order.
fn section(accounts: &[Account], balances: &Balances, keep: impl Fn(AccountSubtype) -> bool) -> Vec<ReportLine> {
    sorted(accounts)
        .into_iter()
        .filter(|a| keep(a.subtype))
        .filter_map(|a| {
            let amount = a.kind().natural(raw(balances, a.id));
            (!amount.is_zero()).then(|| ReportLine {
                account_id: a.id,
                code: a.code.clone(),
                name: a.name.clone(),
                subtype: a.subtype,
                amount,
            })
        })
        .collect()
}

fn total(lines: &[ReportLine]) -> Cents {
    lines.iter().map(|l| l.amount).sum()
}

/// Income minus expenses for whatever period `balances` covers.
pub fn net_income(accounts: &[Account], balances: &Balances) -> Cents {
    -accounts
        .iter()
        .filter(|a| !a.kind().is_balance_sheet())
        .map(|a| raw(balances, a.id))
        .sum::<Cents>()
}

// ---------------------------------------------------------------- trial balance

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrialBalanceRow {
    pub account_id: AccountId,
    pub code: Option<String>,
    pub name: String,
    pub debit: Cents,
    pub credit: Cents,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrialBalance {
    pub rows: Vec<TrialBalanceRow>,
    pub total_debits: Cents,
    pub total_credits: Cents,
}

impl TrialBalance {
    pub fn is_balanced(&self) -> bool {
        self.total_debits == self.total_credits
    }
}

pub fn trial_balance(accounts: &[Account], balances: &Balances) -> TrialBalance {
    let rows: Vec<TrialBalanceRow> = sorted(accounts)
        .into_iter()
        .filter_map(|a| {
            let r = raw(balances, a.id);
            (!r.is_zero()).then(|| TrialBalanceRow {
                account_id: a.id,
                code: a.code.clone(),
                name: a.name.clone(),
                debit: if r.0 > 0 { r } else { Cents::ZERO },
                credit: if r.0 < 0 { -r } else { Cents::ZERO },
            })
        })
        .collect();
    TrialBalance {
        total_debits: rows.iter().map(|r| r.debit).sum(),
        total_credits: rows.iter().map(|r| r.credit).sum(),
        rows,
    }
}

// ---------------------------------------------------------------- profit & loss

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProfitAndLoss {
    pub income: Vec<ReportLine>,
    pub cost_of_goods_sold: Vec<ReportLine>,
    pub expenses: Vec<ReportLine>,
    pub total_income: Cents,
    pub total_cost_of_goods_sold: Cents,
    pub gross_profit: Cents,
    pub total_expenses: Cents,
    pub net_income: Cents,
}

/// `period_balances` must cover exactly the reporting period.
pub fn profit_and_loss(accounts: &[Account], period_balances: &Balances) -> ProfitAndLoss {
    use AccountSubtype::*;
    let income = section(accounts, period_balances, |t| matches!(t, Income | OtherIncome));
    let cogs = section(accounts, period_balances, |t| t == CostOfGoodsSold);
    let expenses = section(accounts, period_balances, |t| matches!(t, Expense | OtherExpense));
    let total_income = total(&income);
    let total_cogs = total(&cogs);
    let total_expenses = total(&expenses);
    let gross_profit = total_income - total_cogs;
    ProfitAndLoss {
        net_income: gross_profit - total_expenses,
        income,
        cost_of_goods_sold: cogs,
        expenses,
        total_income,
        total_cost_of_goods_sold: total_cogs,
        gross_profit,
        total_expenses,
    }
}

// ---------------------------------------------------------------- balance sheet

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BalanceSheet {
    pub assets: Vec<ReportLine>,
    pub liabilities: Vec<ReportLine>,
    /// Equity accounts other than retained earnings.
    pub equity: Vec<ReportLine>,
    /// Earnings from prior fiscal years plus anything posted directly to
    /// retained-earnings accounts. No closing entries are ever needed.
    pub retained_earnings: Cents,
    /// Current fiscal year to date.
    pub net_income: Cents,
    pub total_assets: Cents,
    pub total_liabilities: Cents,
    pub total_equity: Cents,
}

impl BalanceSheet {
    /// Assets = Liabilities + Equity. Always true if every entry balanced.
    pub fn is_balanced(&self) -> bool {
        self.total_assets == self.total_liabilities + self.total_equity
    }
}

/// * `as_of` — cumulative balances through the report date.
/// * `before_fiscal_year` — cumulative balances through the day before the
///   current fiscal year began (empty if the books start inside this year).
pub fn balance_sheet(accounts: &[Account], as_of: &Balances, before_fiscal_year: &Balances) -> BalanceSheet {
    let assets = section(accounts, as_of, |t| t.kind() == AccountKind::Asset);
    let liabilities = section(accounts, as_of, |t| t.kind() == AccountKind::Liability);
    let equity = section(accounts, as_of, |t| t.kind() == AccountKind::Equity && t != AccountSubtype::RetainedEarnings);

    let posted_retained: Cents = section(accounts, as_of, |t| t == AccountSubtype::RetainedEarnings)
        .iter()
        .map(|l| l.amount)
        .sum();
    let prior_earnings = net_income(accounts, before_fiscal_year);
    let net_income = net_income(accounts, as_of) - prior_earnings;
    let retained_earnings = prior_earnings + posted_retained;

    BalanceSheet {
        total_assets: total(&assets),
        total_liabilities: total(&liabilities),
        total_equity: total(&equity) + retained_earnings + net_income,
        assets,
        liabilities,
        equity,
        retained_earnings,
        net_income,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acct(id: AccountId, code: &str, subtype: AccountSubtype) -> Account {
        Account { id, code: Some(code.into()), name: code.into(), subtype, parent_id: None, archived: false }
    }

    fn chart() -> Vec<Account> {
        use AccountSubtype::*;
        vec![
            acct(1, "1000", Bank),
            acct(2, "2000", CreditCard),
            acct(3, "3000", Equity),
            acct(4, "3900", RetainedEarnings),
            acct(5, "4000", Income),
            acct(6, "5000", CostOfGoodsSold),
            acct(7, "6000", Expense),
        ]
    }

    fn bal(pairs: &[(AccountId, i64)]) -> Balances {
        pairs.iter().map(|&(id, c)| (id, Cents(c))).collect()
    }

    #[test]
    fn pnl_sections_and_totals() {
        // Sold 1000, cogs 300, rent 200.
        let b = bal(&[(5, -1000), (6, 300), (7, 200), (1, 500)]);
        let p = profit_and_loss(&chart(), &b);
        assert_eq!(p.total_income, Cents(1000));
        assert_eq!(p.gross_profit, Cents(700));
        assert_eq!(p.total_expenses, Cents(200));
        assert_eq!(p.net_income, Cents(500));
        assert_eq!(net_income(&chart(), &b), Cents(500));
    }

    #[test]
    fn balance_sheet_rolls_prior_years_into_retained_earnings() {
        // Last year: owner put in 10_000, earned 4_000 net (income 5_000, expense 1_000).
        // This year so far: earned 600, put 250 on the credit card for expenses.
        let before = bal(&[(1, 14_000), (3, -10_000), (5, -5_000), (7, 1_000)]);
        let as_of = bal(&[(1, 14_850), (2, -250), (3, -10_000), (5, -5_850), (7, 1_250)]);
        assert_eq!(before.values().copied().sum::<Cents>(), Cents::ZERO);
        assert_eq!(as_of.values().copied().sum::<Cents>(), Cents::ZERO);
        let bs = balance_sheet(&chart(), &as_of, &before);
        assert_eq!(bs.total_assets, Cents(14_850));
        assert_eq!(bs.total_liabilities, Cents(250));
        assert_eq!(bs.retained_earnings, Cents(4_000));
        assert_eq!(bs.net_income, Cents(600));
        assert_eq!(bs.total_equity, Cents(14_600));
        assert!(bs.is_balanced());
    }

    #[test]
    fn trial_balance_splits_sides() {
        let b = bal(&[(1, 500), (5, -500), (3, 0)]);
        let tb = trial_balance(&chart(), &b);
        assert_eq!(tb.rows.len(), 2);
        assert!(tb.is_balanced());
        assert_eq!(tb.total_debits, Cents(500));
    }
}
