// Amounts are written as dollars_cents (e.g. 1200_00), hence the grouping.
#![allow(clippy::inconsistent_digit_grouping)]

use chrono::NaiveDate;
use tally_core::bank::{FeedBatch, FeedTransaction};
use tally_core::rules::{Direction, MatchOp};
use tally_core::{AccountId, AccountSubtype, Cents, EntryLine, NewEntry};
use tally_store::*;

fn d(m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, day).unwrap()
}

fn ft(id: &str, date: NaiveDate, cents: i64, desc: &str) -> FeedTransaction {
    FeedTransaction {
        external_id: id.into(),
        date,
        amount: Cents(cents),
        description: desc.into(),
        merchant: None,
        pending: false,
        category_hint: None,
    }
}

fn added(txns: Vec<FeedTransaction>) -> FeedBatch {
    FeedBatch { added: txns, ..Default::default() }
}

struct Book {
    s: Store,
    checking: AccountId,
    meals: AccountId,
    software: AccountId,
    sales: AccountId,
}

fn book() -> Book {
    let mut s = Store::open_in_memory().unwrap();
    s.seed_default_chart().unwrap();
    let checking = s
        .create_account(&NewAccount::new("Checking", AccountSubtype::Bank).code("1000"))
        .unwrap();
    let id = |s: &Store, c: &str| s.account_by_code(c).unwrap().unwrap().id;
    Book { meals: id(&s, "6250"), software: id(&s, "6500"), sales: id(&s, "4000"), s, checking }
}

#[test]
fn import_is_idempotent_and_respects_start_date() {
    let mut b = book();
    let conn = b.s.create_connection(Provider::Plaid, "First Platypus", Some("item-1"), Some("plaid:item-1")).unwrap();
    b.s.link_account(conn, "acct-1", b.checking, "Plaid Checking", Some("0000"), d(1, 10)).unwrap();

    let batch = added(vec![ft("old", d(1, 5), -100, "BEFORE START"), ft("t1", d(1, 12), -4500, "FIGMA"), ft("t2", d(1, 13), 2500_00, "CLIENT ACH")]);
    let s1 = b.s.import_feed(b.checking, &batch).unwrap();
    assert_eq!((s1.added, s1.skipped), (2, 1));
    let s2 = b.s.import_feed(b.checking, &batch).unwrap();
    assert_eq!(s2.added, 0);
    assert_eq!(b.s.inbox(None).unwrap().len(), 2);
}

#[test]
fn accept_posts_a_cleared_entry_and_undo_voids_it() {
    let mut b = book();
    b.s.import_feed(b.checking, &added(vec![ft("t1", d(2, 1), -45_00, "FIGMA MONTHLY")])).unwrap();
    let item = &b.s.inbox(None).unwrap()[0];
    let entry = b.s.accept(item.txn.id, b.software, None).unwrap();

    assert_eq!(b.s.account_balance(b.software, d(2, 28)).unwrap(), Cents(45_00));
    assert_eq!(b.s.account_balance(b.checking, d(2, 28)).unwrap(), Cents(-45_00));
    let reg = b.s.register(b.checking).unwrap();
    assert_eq!(reg[0].cleared, Cleared::Cleared, "feed postings are pre-cleared");
    assert_eq!(b.s.entry(entry).unwrap().source, "bank_feed");
    assert!(b.s.inbox(None).unwrap().is_empty());

    b.s.undo_review(item.txn.id).unwrap();
    assert_eq!(b.s.account_balance(b.software, d(2, 28)).unwrap(), Cents(0));
    assert_eq!(b.s.inbox(None).unwrap().len(), 1);
    // And it can be re-categorized.
    b.s.accept(item.txn.id, b.meals, Some("Team lunch".into())).unwrap();
    assert_eq!(b.s.account_balance(b.meals, d(2, 28)).unwrap(), Cents(45_00));
}

#[test]
fn split_must_sum_to_the_transaction() {
    let mut b = book();
    b.s.import_feed(b.checking, &added(vec![ft("t1", d(2, 1), -100_00, "COSTCO")])).unwrap();
    let id = b.s.inbox(None).unwrap()[0].txn.id;
    let bad = [Split { account_id: b.meals, amount: Cents(-60_00), memo: None }];
    assert!(matches!(b.s.accept_split(id, &bad, None), Err(StoreError::Rule(_))));
    let good = [
        Split { account_id: b.meals, amount: Cents(-60_00), memo: Some("snacks".into()) },
        Split { account_id: b.software, amount: Cents(-40_00), memo: None },
    ];
    let e = b.s.accept_split(id, &good, None).unwrap();
    assert_eq!(b.s.entry(e).unwrap().lines.len(), 3);
}

#[test]
fn suggests_and_performs_a_match_with_an_existing_entry() {
    let mut b = book();
    // A check written and entered by hand on 3/1...
    let manual = b
        .s
        .post_entry(&NewEntry::new(d(3, 1), "Check 1042 - landlord").line(EntryLine::debit(b.meals, Cents(1200_00))).line(EntryLine::credit(b.checking, Cents(1200_00))))
        .unwrap();
    // ...clears the bank on 3/4.
    b.s.import_feed(b.checking, &added(vec![ft("chk", d(3, 4), -1200_00, "CHECK 1042")])).unwrap();
    let item = b.s.inbox(None).unwrap().remove(0);
    let Some(Suggestion::Match { line_id, entry_id, .. }) = item.suggestion else { panic!("expected match, got {:?}", item.suggestion) };
    assert_eq!(entry_id, manual);

    b.s.match_txn(item.txn.id, line_id).unwrap();
    assert_eq!(b.s.account_balance(b.checking, d(3, 31)).unwrap(), Cents(-1200_00), "no duplicate");
    assert_eq!(b.s.register(b.checking).unwrap()[0].cleared, Cleared::Cleared);

    b.s.undo_review(item.txn.id).unwrap();
    assert_eq!(b.s.register(b.checking).unwrap()[0].cleared, Cleared::Uncleared);
}

#[test]
fn rules_suggest_and_auto_post() {
    let mut b = book();
    let rule = NewRule {
        name: "Stripe payouts".into(),
        priority: 10,
        op: MatchOp::Contains,
        pattern: "stripe".into(),
        direction: Direction::Inflow,
        amount_min: None,
        amount_max: None,
        bank_account_id: None,
        target_account_id: b.sales,
        rename_to: Some("Stripe payout".into()),
        auto_post: true,
        person_id: None,
    };
    b.s.create_rule(&rule).unwrap();
    b.s.create_rule(&NewRule { name: "SaaS".into(), pattern: "notion".into(), direction: Direction::Outflow, target_account_id: b.software, auto_post: false, rename_to: None, ..rule.clone() })
        .unwrap();

    let s = b
        .s
        .import_feed(b.checking, &added(vec![ft("p1", d(4, 1), 980_00, "STRIPE TRANSFER ST-XYZ"), ft("n1", d(4, 2), -10_00, "NOTION LABS")]))
        .unwrap();
    assert_eq!(s.auto_posted, 1);
    assert_eq!(b.s.account_balance(b.sales, d(4, 30)).unwrap(), Cents(980_00));

    let inbox = b.s.inbox(None).unwrap();
    assert_eq!(inbox.len(), 1);
    assert!(matches!(inbox[0].suggestion, Some(Suggestion::Rule { account_id, .. }) if account_id == b.software));
}

#[test]
fn learns_categories_from_history() {
    let mut b = book();
    b.s.import_feed(b.checking, &added(vec![ft("c1", d(5, 1), -6_75, "STARBUCKS 0423")])).unwrap();
    let first = b.s.inbox(None).unwrap()[0].txn.id;
    b.s.accept(first, b.meals, None).unwrap();

    b.s.import_feed(b.checking, &added(vec![ft("c2", d(5, 8), -7_10, "STARBUCKS 0517")])).unwrap();
    let item = &b.s.inbox(None).unwrap()[0];
    assert_eq!(item.suggestion, Some(Suggestion::History { account_id: b.meals }));
}

#[test]
fn pending_and_removed_transactions() {
    let mut b = book();
    let mut p = ft("pend", d(6, 1), -20_00, "UBER PENDING");
    p.pending = true;
    b.s.import_feed(b.checking, &added(vec![p.clone()])).unwrap();
    let id = b.s.inbox(None).unwrap()[0].txn.id;
    assert!(b.s.accept(id, b.meals, None).is_err(), "can't post pending");
    assert_eq!(b.s.inbox_count().unwrap(), 0, "pending doesn't count as to-do");

    // Plaid swaps the pending row for a posted one with a new id.
    b.s.import_feed(b.checking, &FeedBatch { added: vec![ft("posted", d(6, 2), -21_50, "UBER TRIP")], modified: vec![], removed: vec!["pend".into()] })
        .unwrap();
    let inbox = b.s.inbox(None).unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].txn.amount, Cents(-21_50));
}

#[test]
fn exclude_and_restore() {
    let mut b = book();
    b.s.import_feed(b.checking, &added(vec![ft("dup", d(7, 1), -5_00, "DUPLICATE")])).unwrap();
    let id = b.s.inbox(None).unwrap()[0].txn.id;
    b.s.exclude(id).unwrap();
    assert!(b.s.inbox(None).unwrap().is_empty());
    assert_eq!(b.s.reviewed(None, 10).unwrap().len(), 1);
    b.s.undo_review(id).unwrap();
    assert_eq!(b.s.inbox(None).unwrap().len(), 1);
}

#[test]
fn opening_balance_feeds_reconciliation() {
    let mut b = book();
    b.s.post_opening_balance(b.checking, d(1, 1), Cents(5000_00)).unwrap();
    let bs = b.s.balance_sheet(d(1, 1)).unwrap();
    assert!(bs.is_balanced());
    let rec = b.s.start_reconciliation(b.checking, d(1, 31), Cents(5000_00)).unwrap();
    assert!(b.s.reconciliation_summary(rec).unwrap().is_balanced(), "opening line is pre-checked");
}

#[test]
fn only_bank_like_accounts_link() {
    let b = book();
    let conn = b.s.create_connection(Provider::Simplefin, "CU", None, None).unwrap();
    assert!(b.s.link_account(conn, "x", b.meals, "x", None, d(1, 1)).is_err());
    b.s.link_account(conn, "x", b.checking, "x", None, d(1, 1)).unwrap();
    assert!(b.s.link_account(conn, "y", b.checking, "y", None, d(1, 1)).is_err(), "one feed per account");
}
