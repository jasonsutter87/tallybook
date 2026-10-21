// Amounts are written as dollars_cents (e.g. 1200_00), hence the grouping.
#![allow(clippy::inconsistent_digit_grouping)]

//! Bringing a household over from Ledger (mels-budget-app), against a
//! database with Ledger's own schema.

use chrono::NaiveDate;
use rusqlite::Connection;
use std::path::PathBuf;
use tally_core::budget::Direction;
use tally_core::schedule::Cadence;
use tally_core::{Cents, Flex, Scope};
use tally_store::{BudgetTarget, Store};

fn d(m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, day).unwrap()
}

/// A small Ledger database: Mel and Ethan, a few sectors and stores, two
/// months of entries, a plan, a bill, and a goal with contributions.
fn ledger(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tally-ledger-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("ledger.db");
    let c = Connection::open(&path).unwrap();
    c.execute_batch(include_str!("fixtures/ledger_schema.sql")).unwrap();
    c.execute_batch(
        "INSERT INTO person VALUES ('p-mel', 'Mel', 'teal', 0, NULL), ('p-ethan', 'Ethan', 'amber', 1, NULL), ('p-old', 'Old', 'slate', 2, '2026-01-01');
         INSERT INTO sector(id, name, sort_order, changeability) VALUES
           ('s-groc', 'Groceries', 0, 'medium'), ('s-dine', 'Dining', 1, 'high'),
           ('s-kids', 'Kids', 2, 'high'), ('s-save', 'Savings', 3, 'low'), ('s-yoga', 'Yoga', 4, 'high'), ('s-bills', 'Bills', 5, 'low');
         INSERT INTO store VALUES ('st-safe', 'Safeway', 's-groc', NULL, 'safeway', 4, NULL, NULL),
           ('st-rob', 'Roblox', 's-kids', 'p-ethan', 'roblox', 2, NULL, NULL);
         INSERT INTO month VALUES ('2026-08', 450000, '2026-08-01', NULL), ('2026-09', 0, '2026-09-01', NULL);
         INSERT INTO allocation VALUES ('2026-08', 'p-mel', 300000, 'ceiling', 10), ('2026-08', 'p-ethan', 20000, 'ceiling', 10),
           ('2026-09', 'p-mel', 30000, 'floor', 10);
         INSERT INTO goal VALUES ('g-hi', 'Hawaii', 300000, '2027-06-01', 'family trip', '2026-08-01', NULL, NULL);
         INSERT INTO recurring VALUES ('r-phone', 'p-mel', 'Verizon', 16088, 'monthly', NULL, 11, NULL, 1, 's-bills', 1),
           ('r-allow', 'p-ethan', 'Allowance', 1000, 'weekly', 5, NULL, NULL, 1, 's-kids', 0);
         INSERT INTO entry(id, month_id, person_id, occurred_on, amount_cents, note, source, external_id, created_at, store_id, sector_id, goal_id) VALUES
           ('e1', '2026-08', NULL, '2026-08-01', 320000, 'pay', 'manual', NULL, '1', NULL, NULL, NULL),
           ('e2', '2026-08', 'p-mel', '2026-08-03', -8420, 'weekly shop', 'manual', NULL, '2', 'st-safe', 's-groc', NULL),
           ('e3', '2026-08', 'p-ethan', '2026-08-04', -999, '', 'manual', NULL, '3', 'st-rob', 's-kids', NULL),
           ('e4', '2026-08', 'p-mel', '2026-08-05', -4000, 'class', 'manual', NULL, '4', NULL, 's-yoga', NULL),
           ('e5', '2026-08', NULL, '2026-08-06', 1500, 'returned', 'manual', NULL, '5', 'st-safe', 's-groc', NULL),
           ('e6', '2026-08', NULL, '2026-08-20', 25000, 'toward Hawaii', 'manual', NULL, '6', NULL, NULL, 'g-hi'),
           ('e7', '2026-09', NULL, '2026-09-03', -5000, '', 'manual', NULL, '7', NULL, NULL, NULL),
           ('e8', '2026-09', NULL, '2026-09-20', 20000, 'toward Hawaii', 'manual', NULL, '8', NULL, NULL, 'g-hi');",
    )
    .unwrap();
    path
}

fn household() -> Store {
    let mut s = Store::open_in_memory().unwrap();
    s.add_side(Scope::Business).unwrap();
    s.add_side(Scope::Household).unwrap();
    s
}

#[test]
fn preview_counts_what_is_there() {
    let p = Store::preview_ledger(&ledger("preview")).unwrap();
    assert_eq!((p.people, p.entries, p.months, p.goals, p.bills, p.stores), (3, 8, 2, 1, 2, 2));
    assert_eq!((p.first, p.last), (Some(d(8, 1)), Some(d(9, 20))));
}

#[test]
fn a_ledger_comes_over_whole() {
    let path = ledger("whole");
    let mut s = household();
    let r = s.import_ledger(&path, None).unwrap();
    assert_eq!((r.people, r.entries, r.goals, r.bills, r.rules, r.months), (3, 8, 1, 2, 2, 2));
    assert_eq!(r.categories_created, 2, "Yoga and the catch-all Bills are new; the rest match the household chart");
    assert_eq!(r.notes.len(), 1, "the person floor is explained: {:?}", r.notes);

    let accounts = s.accounts_in(Some(Scope::Household)).unwrap();
    let yoga = accounts.iter().find(|a| a.name == "Yoga").unwrap();
    assert_eq!(yoga.flex, Some(Flex::Lifestyle));
    assert_eq!(accounts.iter().find(|a| a.name == "Bills").unwrap().flex, Some(Flex::Fixed), "low changeability");
    let people = s.people().unwrap();
    let mel = people.iter().find(|p| p.name == "Mel").unwrap().id;
    let ethan = people.iter().find(|p| p.name == "Ethan").unwrap().id;
    assert!(people.iter().find(|p| p.name == "Old").unwrap().archived);

    // August: $3,200 in, groceries $84.20 less a $15 return, Roblox for Ethan, yoga for Mel.
    let aug = s.month_budget("2026-08", d(9, 30)).unwrap();
    assert_eq!(aug.income, Cents(3200_00));
    assert_eq!(aug.spending, Cents(84_20 - 15_00 + 9_99 + 40_00));
    assert_eq!(aug.available, Some(Cents(4500_00)));
    let line = |t| aug.lines.iter().find(|l| l.target == t).unwrap();
    assert_eq!(line(BudgetTarget::Person(mel)).amount, Cents(3000_00));
    assert_eq!(line(BudgetTarget::Person(ethan)).progress, Cents(9_99));
    let groceries = aug.categories.iter().find(|c| c.name == "Groceries").unwrap();
    assert_eq!(groceries.spending, Cents(69_20));

    // Both sides of the book still balance.
    for scope in [Some(Scope::Household), Some(Scope::Business), None] {
        assert!(s.balance_sheet_in(d(12, 31), scope).unwrap().is_balanced());
    }

    // The goal kept its contributions, in a real savings account.
    let goal = &s.goals(d(10, 1)).unwrap()[0];
    assert_eq!((goal.goal.name.as_str(), goal.saved, goal.contributions), ("Hawaii", Cents(450_00), 2));
    let savings = s.account(goal.goal.account_id).unwrap();
    assert_eq!(s.account_balance(savings.id, d(12, 31)).unwrap(), Cents(450_00));

    // Bills, with their cadence and auto-pay.
    let bills = s.recurring_bills(d(10, 1)).unwrap();
    let phone = bills.iter().find(|b| b.bill.name == "Verizon").unwrap();
    assert_eq!((phone.bill.cadence, phone.bill.auto_pay, phone.bill.amount), (Cadence::Month { day: 11 }, true, Cents(160_88)));
    let allowance = bills.iter().find(|b| b.bill.name == "Allowance").unwrap();
    assert_eq!(allowance.bill.cadence, Cadence::Week);
    assert_eq!(allowance.bill.start_date, d(8, 7), "Friday (5) on or after August 1st");
    assert_eq!(allowance.bill.person_id, Some(ethan));

    // Stores became rules: Roblox → Kids, for Ethan.
    let rules = s.rules().unwrap();
    let roblox = rules.iter().find(|r| r.pattern == "Roblox").unwrap();
    assert_eq!(roblox.person_id, Some(ethan));
    assert_eq!(s.account(roblox.target_account_id).unwrap().name, "Kids' Activities & Sports");

    // September's person floor wasn't turned into a limit.
    let sep = s.month_budget("2026-09", d(9, 30)).unwrap();
    assert!(sep.lines.iter().all(|l| l.direction == Direction::Ceiling));
    assert_eq!(sep.plan_from, None, "September has its own (empty) plan, as it did in Ledger");
}

#[test]
fn importing_again_adds_only_whats_new() {
    let path = ledger("again");
    let mut s = household();
    s.import_ledger(&path, None).unwrap();
    let again = s.import_ledger(&path, None).unwrap();
    assert_eq!((again.people, again.entries, again.entries_skipped, again.goals, again.bills, again.rules, again.months), (0, 0, 8, 0, 0, 0, 0));

    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "INSERT INTO entry(id, month_id, person_id, occurred_on, amount_cents, note, source, external_id, created_at)
             VALUES ('e9', '2026-09', 'p-mel', '2026-09-25', -2500, 'lunch', 'manual', NULL, '9')",
        )
        .unwrap();
    let r = s.import_ledger(&path, None).unwrap();
    assert_eq!((r.entries, r.entries_skipped), (1, 8));
}

#[test]
fn it_needs_a_household_and_a_ledger() {
    let mut business = Store::open_in_memory().unwrap();
    business.seed_default_chart().unwrap();
    assert!(business.import_ledger(&ledger("biz"), None).is_err());

    let not_ledger = std::env::temp_dir().join(format!("tally-not-ledger-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&not_ledger);
    Connection::open(&not_ledger).unwrap().execute_batch("CREATE TABLE x (y)").unwrap();
    let err = Store::preview_ledger(&not_ledger).unwrap_err().to_string();
    assert!(err.contains("doesn't look like a Ledger database"), "{err}");
}

#[test]
fn money_can_land_in_a_chosen_account() {
    let mut s = household();
    let checking = s.create_account(&tally_store::NewAccount::new("Golden 1 Checking", tally_core::AccountSubtype::Bank).scope(Scope::Household)).unwrap();
    let r = s.import_ledger(&ledger("chosen"), Some(checking)).unwrap();
    assert_eq!(r.money_account_id, checking);
    assert!(s.accounts().unwrap().iter().all(|a| a.name != "Ledger import"));
}
