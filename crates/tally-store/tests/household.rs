// Amounts are written as dollars_cents (e.g. 1200_00), hence the grouping.
#![allow(clippy::inconsistent_digit_grouping)]

use chrono::NaiveDate;
use tally_core::chart::codes;
use tally_core::{AccountId, AccountSubtype, Cents, EntryLine, Flex, NewEntry, Scope};
use tally_store::{NewAccount, Store};

fn d(m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, day).unwrap()
}

fn code(s: &Store, c: &str) -> AccountId {
    s.account_by_code(c).unwrap().unwrap().id
}

/// A sole proprietor who keeps the business and the household in one book.
struct Both {
    s: Store,
    biz_checking: AccountId,
    biz_card: AccountId,
    home_checking: AccountId,
}

fn both() -> Both {
    let mut s = Store::open_in_memory().unwrap();
    s.add_side(Scope::Business).unwrap();
    s.add_side(Scope::Household).unwrap();
    let biz_checking = s.create_account(&NewAccount::new("Business Checking", AccountSubtype::Bank).code("1000")).unwrap();
    let biz_card = s.create_account(&NewAccount::new("Chase Ink", AccountSubtype::CreditCard).code("2000")).unwrap();
    let home_checking = s
        .create_account(&NewAccount::new("Golden 1 Checking", AccountSubtype::Bank).code("H1000").scope(Scope::Household))
        .unwrap();
    Both { s, biz_checking, biz_card, home_checking }
}

fn post(s: &mut Store, date: NaiveDate, memo: &str, debit: AccountId, credit: AccountId, cents: i64) -> i64 {
    s.post_entry(&NewEntry::new(date, memo).line(EntryLine::debit(debit, Cents(cents))).line(EntryLine::credit(credit, Cents(cents))))
        .unwrap()
}

#[test]
fn a_household_book_has_only_household_accounts() {
    let mut s = Store::open_in_memory().unwrap();
    s.add_side(Scope::Household).unwrap();
    assert_eq!(s.sides().unwrap(), vec![Scope::Household]);
    let accounts = s.accounts().unwrap();
    assert!(accounts.iter().all(|a| a.scope == Scope::Household));
    let groceries = accounts.iter().find(|a| a.name == "Groceries").unwrap();
    assert_eq!(groceries.flex, Some(Flex::Usage));
    // No business side, so no business accounts can be added.
    assert!(s.create_account(&NewAccount::new("Biz Checking", AccountSubtype::Bank)).is_err());
}

#[test]
fn books_from_before_households_are_business_books() {
    let mut s = Store::open_in_memory().unwrap();
    s.seed_default_chart().unwrap();
    assert_eq!(s.sides().unwrap(), vec![Scope::Business]);
    assert!(s.accounts().unwrap().iter().all(|a| a.scope == Scope::Business));
}

#[test]
fn adding_a_household_side_keeps_the_business() {
    let mut s = Store::open_in_memory().unwrap();
    s.seed_default_chart().unwrap();
    let n = s.accounts().unwrap().len();
    s.add_side(Scope::Household).unwrap();
    s.add_side(Scope::Household).unwrap();
    assert_eq!(s.sides().unwrap(), vec![Scope::Business, Scope::Household]);
    let household = s.accounts_in(Some(Scope::Household)).unwrap();
    assert_eq!(s.accounts().unwrap().len(), n + household.len());
}

#[test]
fn groceries_on_the_business_card_become_an_owners_draw() {
    let mut b = both();
    let groceries = code(&b.s, "H6200");
    let id = post(&mut b.s, d(3, 4), "Safeway", groceries, b.biz_card, 84_20);

    let e = b.s.entry(id).unwrap();
    assert_eq!(e.lines.len(), 4);
    let line = |acct: AccountId| e.lines.iter().find(|l| l.account_id == acct).unwrap().amount;
    assert_eq!(line(code(&b.s, codes::OWNERS_DRAW)), Cents(84_20));
    assert_eq!(line(code(&b.s, codes::FROM_THE_BUSINESS)), Cents(-84_20));

    // The business sees a draw, not groceries; the household sees both.
    let biz = b.s.profit_and_loss_in(d(1, 1), d(12, 31), Some(Scope::Business)).unwrap();
    assert_eq!(biz.net_income, Cents::ZERO);
    let home = b.s.profit_and_loss_in(d(1, 1), d(12, 31), Some(Scope::Household)).unwrap();
    assert_eq!(home.total_income, Cents(84_20));
    assert_eq!(home.total_expenses, Cents(84_20));
}

#[test]
fn moving_money_home_and_back() {
    let mut b = both();
    let services = code(&b.s, "4100");
    post(&mut b.s, d(1, 2), "Client payment", b.biz_checking, services, 5000_00);
    post(&mut b.s, d(1, 15), "Pay myself", b.home_checking, b.biz_checking, 3000_00);
    post(&mut b.s, d(1, 20), "Cover a slow month", b.biz_checking, b.home_checking, 500_00);

    let home = b.s.profit_and_loss_in(d(1, 1), d(1, 31), Some(Scope::Household)).unwrap();
    assert_eq!(home.total_income, Cents(3000_00));
    assert_eq!(home.total_expenses, Cents(500_00));

    let biz = b.s.profit_and_loss_in(d(1, 1), d(1, 31), Some(Scope::Business)).unwrap();
    assert_eq!(biz.net_income, Cents(5000_00), "draws and contributions aren't business expenses or income");

    for scope in [None, Some(Scope::Business), Some(Scope::Household)] {
        assert!(b.s.balance_sheet_in(d(1, 31), scope).unwrap().is_balanced(), "{scope:?}");
        assert!(b.s.trial_balance_in(d(1, 31), scope).unwrap().is_balanced(), "{scope:?}");
    }
    assert_eq!(b.s.account_balance(b.home_checking, d(1, 31)).unwrap(), Cents(2500_00));
}

#[test]
fn voiding_a_crossing_entry_reverses_the_draw_too() {
    let mut b = both();
    let shopping = code(&b.s, "H6720");
    let id = post(&mut b.s, d(2, 1), "Target", shopping, b.biz_card, 40_00);
    b.s.void_entry(id, d(2, 2)).unwrap();
    let draw = code(&b.s, codes::OWNERS_DRAW);
    assert_eq!(b.s.account_balance(draw, d(2, 28)).unwrap(), Cents::ZERO);
    assert!(b.s.balance_sheet_in(d(2, 28), Some(Scope::Household)).unwrap().is_balanced());
}

#[test]
fn a_one_sided_book_cannot_cross() {
    let mut s = Store::open_in_memory().unwrap();
    s.add_side(Scope::Household).unwrap();
    let checking = s.create_account(&NewAccount::new("Checking", AccountSubtype::Bank).scope(Scope::Household)).unwrap();
    // Ordinary household spending needs no bridge.
    let rent = code(&s, "H6000");
    post(&mut s, d(1, 3), "Rent", rent, checking, 1495_00);
    assert!(s.balance_sheet_in(d(1, 31), Some(Scope::Household)).unwrap().is_balanced());
}

#[test]
fn household_opening_balances_use_household_equity() {
    let mut b = both();
    b.s.post_opening_balance(b.home_checking, d(1, 1), Cents(1098_13)).unwrap();
    let equity = code(&b.s, codes::HOUSEHOLD_OPENING_BALANCE);
    assert_eq!(b.s.account_balance(equity, d(1, 1)).unwrap(), Cents(1098_13));
    assert!(b.s.balance_sheet_in(d(1, 1), Some(Scope::Household)).unwrap().is_balanced());
    assert!(b.s.balance_sheet_in(d(1, 1), Some(Scope::Business)).unwrap().is_balanced());
}

#[test]
fn household_categories_stay_off_schedule_c() {
    let mut b = both();
    let groceries = code(&b.s, "H6200");
    post(&mut b.s, d(4, 1), "Groceries", groceries, b.home_checking, 120_00);
    let sc = b.s.schedule_c(2026, Cents::ZERO).unwrap();
    assert!(sc.rows.iter().all(|r| !r.name.contains("Groceries")));
    assert!(b.s.set_tax_line(groceries, None).is_err());
}

#[test]
fn flex_is_for_household_spending() {
    let b = both();
    let dining = code(&b.s, "H6210");
    b.s.set_flex(dining, Some(Flex::Fixed)).unwrap();
    assert_eq!(b.s.account(dining).unwrap().flex, Some(Flex::Fixed));
    assert!(b.s.set_flex(code(&b.s, "6400"), Some(Flex::Fixed)).is_err());
    assert!(b.s.set_flex(code(&b.s, "H4000"), Some(Flex::Fixed)).is_err());
}

#[test]
fn sub_accounts_stay_on_their_side() {
    let b = both();
    let mut child = NewAccount::new("Costco", AccountSubtype::Expense).scope(Scope::Business);
    child.parent_id = Some(code(&b.s, "H6200"));
    assert!(b.s.create_account(&child).is_err());
}
