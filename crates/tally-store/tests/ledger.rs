// Amounts are written as dollars_cents (e.g. 1200_00), hence the grouping.
#![allow(clippy::inconsistent_digit_grouping)]


use chrono::NaiveDate;
use tally_core::chart::codes;
use tally_core::{AccountId, AccountSubtype, Cents, EntryLine, NewEntry};
use tally_store::{Cleared, NewAccount, Store, StoreError};

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

struct Book {
    store: Store,
    checking: AccountId,
    card: AccountId,
    equity: AccountId,
    sales: AccountId,
    rent: AccountId,
    software: AccountId,
}

fn code(store: &Store, c: &str) -> AccountId {
    store.account_by_code(c).unwrap().unwrap().id
}

fn book() -> Book {
    let mut store = Store::open_in_memory().unwrap();
    store.seed_default_chart().unwrap();
    let checking = store
        .create_account(&NewAccount { code: Some("1000".into()), name: "Checking".into(), subtype: AccountSubtype::Bank, parent_id: None })
        .unwrap();
    let card = store
        .create_account(&NewAccount { code: Some("2000".into()), name: "Visa".into(), subtype: AccountSubtype::CreditCard, parent_id: None })
        .unwrap();
    Book {
        equity: code(&store, "3000"),
        sales: code(&store, "4000"),
        rent: code(&store, "6400"),
        software: code(&store, "6500"),
        store,
        checking,
        card,
    }
}

fn post(store: &mut Store, date: NaiveDate, memo: &str, debit: AccountId, credit: AccountId, cents: i64) -> i64 {
    store
        .post_entry(
            &NewEntry::new(date, memo)
                .line(EntryLine::debit(debit, Cents(cents)))
                .line(EntryLine::credit(credit, Cents(cents))),
        )
        .unwrap()
}

#[test]
fn seeding_is_idempotent() {
    let mut b = book();
    let n = b.store.accounts().unwrap().len();
    b.store.seed_default_chart().unwrap();
    assert_eq!(b.store.accounts().unwrap().len(), n);
    assert!(b.store.account_by_code(codes::RETAINED_EARNINGS).unwrap().is_some());
}

#[test]
fn rejects_invalid_entries() {
    let mut b = book();
    let unbalanced = NewEntry::new(d(2026, 1, 1), "bad")
        .line(EntryLine::debit(b.checking, Cents(100)))
        .line(EntryLine::credit(b.sales, Cents(90)));
    assert!(matches!(b.store.post_entry(&unbalanced), Err(StoreError::Entry(_))));

    let ghost = NewEntry::new(d(2026, 1, 1), "ghost")
        .line(EntryLine::debit(b.checking, Cents(100)))
        .line(EntryLine::credit(9999, Cents(100)));
    assert!(matches!(b.store.post_entry(&ghost), Err(StoreError::NotFound("account", 9999))));
    // Nothing partial was written.
    assert!(b.store.register(b.checking).unwrap().is_empty());
}

#[test]
fn history_is_append_only_at_the_sql_level() {
    let mut b = book();
    let id = post(&mut b.store, d(2026, 1, 1), "capital", b.checking, b.equity, 1000_00);
    let line = b.store.entry(id).unwrap().lines[0].id;
    // Bypass the API: the triggers must still refuse.
    let err = b.store_raw_exec(&format!("UPDATE journal_lines SET amount = 1 WHERE id = {line}"));
    assert!(err.contains("append-only"), "{err}");
    let err = b.store_raw_exec(&format!("DELETE FROM journal_entries WHERE id = {id}"));
    assert!(err.contains("append-only"), "{err}");
}

impl Book {
    fn store_raw_exec(&self, sql: &str) -> String {
        self.store.debug_execute(sql).unwrap_err().to_string()
    }
}

#[test]
fn void_posts_a_reversal_and_keeps_history() {
    let mut b = book();
    post(&mut b.store, d(2026, 1, 1), "capital", b.checking, b.equity, 5000_00);
    let rent = post(&mut b.store, d(2026, 1, 5), "Jan rent", b.rent, b.checking, 1200_00);
    assert_eq!(b.store.account_balance(b.checking, d(2026, 1, 31)).unwrap(), Cents(3800_00));

    let void = b.store.void_entry(rent, d(2026, 1, 6)).unwrap();
    assert_eq!(b.store.account_balance(b.checking, d(2026, 1, 31)).unwrap(), Cents(5000_00));
    assert_eq!(b.store.entry(rent).unwrap().voided_by_id, Some(void));
    assert_eq!(b.store.entry(void).unwrap().reverses_id, Some(rent));
    assert_eq!(b.store.register(b.checking).unwrap().len(), 3);

    assert!(matches!(b.store.void_entry(rent, d(2026, 1, 7)), Err(StoreError::Rule(_))));
    assert!(matches!(b.store.void_entry(void, d(2026, 1, 7)), Err(StoreError::Rule(_))));
}

#[test]
fn lock_date_blocks_old_periods() {
    let mut b = book();
    let jan = post(&mut b.store, d(2026, 1, 15), "jan", b.checking, b.sales, 100_00);
    b.store.set_lock_date(Some(d(2026, 1, 31))).unwrap();

    let late = NewEntry::new(d(2026, 1, 20), "backdated")
        .line(EntryLine::debit(b.checking, Cents(1)))
        .line(EntryLine::credit(b.sales, Cents(1)));
    assert!(matches!(b.store.post_entry(&late), Err(StoreError::Rule(_))));
    // Voiding a locked-period entry is fine as long as the void is dated after the lock.
    assert!(b.store.void_entry(jan, d(2026, 1, 31)).is_err());
    assert!(b.store.void_entry(jan, d(2026, 2, 1)).is_ok());
}

#[test]
fn register_runs_a_natural_balance() {
    let mut b = book();
    post(&mut b.store, d(2026, 2, 1), "Figma", b.software, b.card, 45_00);
    post(&mut b.store, d(2026, 2, 3), "Notion", b.software, b.card, 10_00);
    post(&mut b.store, d(2026, 2, 20), "Pay card", b.card, b.checking, 55_00);
    let rows = b.store.register(b.card).unwrap();
    let balances: Vec<Cents> = rows.iter().map(|r| r.balance).collect();
    // Credit card shows amount owed: up with charges, down with payments.
    assert_eq!(balances, vec![Cents(45_00), Cents(55_00), Cents(0)]);
}

#[test]
fn reports_tie_out_across_a_fiscal_year_boundary() {
    let mut b = book();
    // 2025: owner invests 10k, earns 5k, pays 1k rent.
    post(&mut b.store, d(2025, 3, 1), "capital", b.checking, b.equity, 10_000_00);
    post(&mut b.store, d(2025, 6, 1), "client", b.checking, b.sales, 5_000_00);
    post(&mut b.store, d(2025, 7, 1), "rent", b.rent, b.checking, 1_000_00);
    // 2026: earns 850, 250 of software on the card.
    post(&mut b.store, d(2026, 1, 10), "client", b.checking, b.sales, 850_00);
    post(&mut b.store, d(2026, 1, 12), "Figma", b.software, b.card, 250_00);

    let tb = b.store.trial_balance(d(2026, 12, 31)).unwrap();
    assert!(tb.is_balanced());

    let pnl = b.store.profit_and_loss(d(2026, 1, 1), d(2026, 12, 31)).unwrap();
    assert_eq!(pnl.total_income, Cents(850_00));
    assert_eq!(pnl.total_expenses, Cents(250_00));
    assert_eq!(pnl.net_income, Cents(600_00));

    let bs = b.store.balance_sheet(d(2026, 6, 30)).unwrap();
    assert!(bs.is_balanced());
    assert_eq!(bs.total_assets, Cents(14_850_00));
    assert_eq!(bs.total_liabilities, Cents(250_00));
    assert_eq!(bs.retained_earnings, Cents(4_000_00));
    assert_eq!(bs.net_income, Cents(600_00));

    // A July fiscal year moves the split: Jul 2025–Jun 2026 is now "this year".
    b.store.set_fiscal_year_start_month(7).unwrap();
    let bs = b.store.balance_sheet(d(2026, 6, 30)).unwrap();
    assert!(bs.is_balanced());
    assert_eq!(bs.retained_earnings, Cents(5_000_00));
    assert_eq!(bs.net_income, Cents(-400_00));
}

#[test]
fn reconciliation_happy_path_and_guards() {
    let mut b = book();
    post(&mut b.store, d(2026, 3, 1), "opening", b.checking, b.equity, 1000_00);
    post(&mut b.store, d(2026, 3, 10), "deposit", b.checking, b.sales, 500_00);
    post(&mut b.store, d(2026, 3, 15), "rent", b.rent, b.checking, 200_00);
    post(&mut b.store, d(2026, 3, 30), "check not yet cashed", b.rent, b.checking, 75_00);
    post(&mut b.store, d(2026, 4, 2), "after statement", b.checking, b.sales, 1_00);

    // Statement through 3/31 shows 1300 (the 75 check hasn't cleared).
    let rec = b.store.start_reconciliation(b.checking, d(2026, 3, 31), Cents(1300_00)).unwrap();
    let cands = b.store.reconcile_candidates(rec).unwrap();
    assert_eq!(cands.len(), 4, "the April line is excluded");
    assert!(b.store.start_reconciliation(b.checking, d(2026, 3, 31), Cents(0)).is_err(), "one open session per account");

    for c in cands.iter().filter(|c| c.amount != Cents(-75_00)) {
        b.store.set_cleared(rec, c.line_id, true).unwrap();
    }
    let s = b.store.reconciliation_summary(rec).unwrap();
    assert_eq!(s.cleared_balance, Cents(1300_00));
    assert!(s.is_balanced());

    // Can't finish when off; uncheck one to prove it.
    let deposit = cands.iter().find(|c| c.amount == Cents(500_00)).unwrap().line_id;
    b.store.set_cleared(rec, deposit, false).unwrap();
    assert!(matches!(b.store.finish_reconciliation(rec), Err(StoreError::Rule(_))));
    b.store.set_cleared(rec, deposit, true).unwrap();
    b.store.finish_reconciliation(rec).unwrap();

    // Reconciled lines are now protected.
    let rent_entry = cands.iter().find(|c| c.amount == Cents(-200_00)).unwrap().entry_id;
    assert!(matches!(b.store.void_entry(rent_entry, d(2026, 4, 5)), Err(StoreError::Rule(_))));
    let reg = b.store.register(b.checking).unwrap();
    assert_eq!(reg.iter().filter(|r| r.cleared == Cleared::Reconciled).count(), 3);

    // Next month starts from the reconciled 1300.
    let rec2 = b.store.start_reconciliation(b.checking, d(2026, 4, 30), Cents(1226_00)).unwrap();
    let s2 = b.store.reconciliation_summary(rec2).unwrap();
    assert_eq!(s2.beginning_balance, Cents(1300_00));
    for c in b.store.reconcile_candidates(rec2).unwrap() {
        b.store.set_cleared(rec2, c.line_id, true).unwrap();
    }
    b.store.finish_reconciliation(rec2).unwrap();

    // Undo peels back the most recent statement only.
    b.store.undo_last_reconciliation(b.checking).unwrap();
    let reg = b.store.register(b.checking).unwrap();
    assert_eq!(reg.iter().filter(|r| r.cleared == Cleared::Reconciled).count(), 3);
    assert_eq!(reg.iter().filter(|r| r.cleared == Cleared::Cleared).count(), 2);
}

#[test]
fn bank_cleared_lines_are_prechecked_and_voids_net_to_zero() {
    let mut b = book();
    post(&mut b.store, d(2026, 5, 1), "opening", b.checking, b.equity, 100_00);
    let oops = post(&mut b.store, d(2026, 5, 2), "duplicate", b.rent, b.checking, 40_00);
    // Simulate a bank-feed match clearing the opening line outside a reconciliation.
    let opening_line = b.store.register(b.checking).unwrap()[0].line_id;
    b.store.debug_execute(&format!("UPDATE journal_lines SET cleared = 'cleared' WHERE id = {opening_line}")).unwrap();

    let rec = b.store.start_reconciliation(b.checking, d(2026, 5, 31), Cents(100_00)).unwrap();
    let oops_line = b.store.entry(oops).unwrap().lines[1].id;
    b.store.set_cleared(rec, oops_line, true).unwrap();
    b.store.void_entry(oops, d(2026, 5, 3)).unwrap();

    let s = b.store.reconciliation_summary(rec).unwrap();
    assert!(s.is_balanced(), "{s:?}");
    b.store.finish_reconciliation(rec).unwrap();
}

#[test]
fn archived_accounts_reject_new_entries() {
    let mut b = book();
    let travel = code(&b.store, "6600");
    b.store.set_archived(travel, true).unwrap();
    let e = NewEntry::new(d(2026, 1, 1), "trip")
        .line(EntryLine::debit(travel, Cents(1)))
        .line(EntryLine::credit(b.checking, Cents(1)));
    assert!(matches!(b.store.post_entry(&e), Err(StoreError::Rule(_))));

    post(&mut b.store, d(2026, 1, 1), "rent", b.rent, b.checking, 1);
    assert!(b.store.set_archived(b.rent, true).is_err(), "non-zero balance");
}
