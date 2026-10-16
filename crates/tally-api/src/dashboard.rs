//! The home screen's numbers, in one round trip.

use chrono::{Datelike, Months, NaiveDate};
use serde::Serialize;
use tally_core::{AccountId, AccountKind, AccountSubtype, Cents};
use tally_store::{DocStatus, Result, Store};

#[derive(Debug, Serialize)]
pub struct MoneyAccount {
    pub id: AccountId,
    pub name: String,
    pub subtype: AccountSubtype,
    /// Natural sign: what you have (bank) or owe (card).
    pub balance: Cents,
    pub bank_balance: Option<Cents>,
    pub mask: Option<String>,
    pub linked: bool,
}

#[derive(Debug, Serialize)]
pub struct Period {
    pub income: Cents,
    pub expenses: Cents,
    pub net: Cents,
}

#[derive(Debug, Serialize)]
pub struct MonthFlow {
    /// `2026-05`
    pub month: String,
    pub income: Cents,
    pub expenses: Cents,
}

#[derive(Debug, Serialize)]
pub struct Category {
    pub account_id: AccountId,
    pub name: String,
    pub amount: Cents,
}

#[derive(Debug, Serialize)]
pub struct CountAmount {
    pub count: usize,
    pub amount: Cents,
}

#[derive(Debug, Serialize)]
pub struct Dashboard {
    pub today: NaiveDate,
    pub cash: Cents,
    pub credit_owed: Cents,
    pub receivable: Cents,
    pub payable: Cents,
    pub accounts: Vec<MoneyAccount>,
    pub this_month: Period,
    pub last_month: Period,
    pub cashflow: Vec<MonthFlow>,
    pub top_expenses: Vec<Category>,
    pub inbox_count: i64,
    pub overdue_invoices: CountAmount,
    pub bills_due_soon: CountAmount,
}

fn month_start(d: NaiveDate) -> NaiveDate {
    d.with_day(1).expect("day 1 exists")
}

fn month_end(d: NaiveDate) -> NaiveDate {
    month_start(d) + Months::new(1) - chrono::Days::new(1)
}

fn period(store: &Store, from: NaiveDate, to: NaiveDate) -> Result<Period> {
    let p = store.profit_and_loss(from, to)?;
    let expenses = p.total_cost_of_goods_sold + p.total_expenses;
    Ok(Period { income: p.total_income, expenses, net: p.net_income })
}

pub fn build(store: &Store, today: NaiveDate) -> Result<Dashboard> {
    let accounts = store.accounts()?;
    let balances = store.balances(None, today)?;
    let natural = |a: &tally_core::Account| a.kind().natural(balances.get(&a.id).copied().unwrap_or_default());
    let sum_of = |pred: &dyn Fn(AccountSubtype) -> bool| -> Cents {
        accounts.iter().filter(|a| pred(a.subtype)).map(natural).sum()
    };

    let mut money = Vec::new();
    for a in accounts.iter().filter(|a| !a.archived && matches!(a.subtype, AccountSubtype::Bank | AccountSubtype::CreditCard)) {
        let link = store.link_for_account(a.id)?;
        money.push(MoneyAccount {
            id: a.id,
            name: a.name.clone(),
            subtype: a.subtype,
            balance: natural(a),
            bank_balance: link.as_ref().and_then(|l| l.bank_balance),
            mask: link.as_ref().and_then(|l| l.mask.clone()),
            linked: link.is_some(),
        });
    }

    let this_start = month_start(today);
    let last_start = this_start - Months::new(1);
    let mut cashflow = Vec::new();
    for back in (0..6).rev() {
        let start = this_start - Months::new(back);
        let p = period(store, start, month_end(start))?;
        cashflow.push(MonthFlow { month: start.format("%Y-%m").to_string(), income: p.income, expenses: p.expenses });
    }

    let month_balances = store.balances(Some(this_start), today)?;
    let mut top: Vec<Category> = accounts
        .iter()
        .filter(|a| a.kind() == AccountKind::Expense)
        .filter_map(|a| {
            let amount = month_balances.get(&a.id).copied().unwrap_or_default();
            (amount.0 > 0).then(|| Category { account_id: a.id, name: a.name.clone(), amount })
        })
        .collect();
    top.sort_by(|a, b| b.amount.cmp(&a.amount));
    top.truncate(5);

    let overdue: Vec<_> = store.invoices(today)?.into_iter().filter(|i| i.status == DocStatus::Overdue).collect();
    let soon = today + chrono::Days::new(7);
    let due_soon: Vec<_> = store
        .bills(today)?
        .into_iter()
        .filter(|b| b.balance.0 > 0 && b.due_date <= soon)
        .collect();

    Ok(Dashboard {
        today,
        cash: sum_of(&|t| t == AccountSubtype::Bank),
        credit_owed: sum_of(&|t| t == AccountSubtype::CreditCard),
        receivable: sum_of(&|t| t == AccountSubtype::AccountsReceivable),
        payable: sum_of(&|t| t == AccountSubtype::AccountsPayable),
        accounts: money,
        this_month: period(store, this_start, today)?,
        last_month: period(store, last_start, this_start - chrono::Days::new(1))?,
        cashflow,
        top_expenses: top,
        inbox_count: store.inbox_count()?,
        overdue_invoices: CountAmount { count: overdue.len(), amount: overdue.iter().map(|i| i.balance).sum() },
        bills_due_soon: CountAmount { count: due_soon.len(), amount: due_soon.iter().map(|b| b.balance).sum() },
    })
}
