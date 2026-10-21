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
fn one_sided_books_need_no_bridge() {
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

// ---------------------------------------------------------------- people & budgets

use tally_core::budget::{Direction, Sentiment, Zone};
use tally_store::{BudgetLine, BudgetTarget, NewPerson};

fn ceiling(target: BudgetTarget, cents: i64) -> BudgetLine {
    BudgetLine { target, amount: Cents(cents), direction: Direction::Ceiling, tolerance_pct: 10 }
}

fn spend(s: &mut Store, date: NaiveDate, category: &str, from: AccountId, cents: i64, person: Option<i64>) -> i64 {
    let category = code(s, category);
    let id = post(s, date, "spend", category, from, cents);
    s.set_entry_person(id, person).unwrap();
    id
}

#[test]
fn a_plan_carries_forward_until_a_month_changes_it() {
    let mut b = both();
    let groceries = BudgetTarget::Account(code(&b.s, "H6200"));
    b.s.set_budget_line("2026-03", &ceiling(groceries, 1200_00)).unwrap();
    b.s.set_available("2026-03", Some(Cents(6000_00))).unwrap();

    let may = b.s.month_budget("2026-05", d(5, 10)).unwrap();
    assert_eq!(may.plan_from.as_deref(), Some("2026-03"));
    assert_eq!(may.lines[0].amount, Cents(1200_00));
    assert_eq!(may.available, Some(Cents(6000_00)));
    assert!(b.s.month_budget("2026-02", d(5, 10)).unwrap().lines.is_empty());

    // Changing May gives May (and what follows) its own plan; March is untouched.
    b.s.set_budget_line("2026-05", &ceiling(groceries, 1000_00)).unwrap();
    assert_eq!(b.s.month_budget("2026-03", d(5, 10)).unwrap().lines[0].amount, Cents(1200_00));
    assert_eq!(b.s.month_budget("2026-04", d(5, 10)).unwrap().lines[0].amount, Cents(1200_00));
    let june = b.s.month_budget("2026-06", d(5, 10)).unwrap();
    assert_eq!(june.lines[0].amount, Cents(1000_00));
    assert_eq!(june.plan_from.as_deref(), Some("2026-05"));
    assert_eq!(june.available, Some(Cents(6000_00)), "the copy keeps what there is to work with");

    // Removing a line in July leaves it off from July on.
    b.s.remove_budget_line("2026-07", groceries).unwrap();
    assert!(b.s.month_budget("2026-08", d(5, 10)).unwrap().lines.is_empty());
    assert_eq!(b.s.month_budget("2026-06", d(5, 10)).unwrap().lines.len(), 1);
}

#[test]
fn a_category_line_knows_how_far_into_the_month_it_is() {
    let mut b = both();
    let groceries = code(&b.s, "H6200");
    b.s.set_budget_line("2026-03", &ceiling(BudgetTarget::Account(groceries), 400_00)).unwrap();
    spend(&mut b.s, d(3, 2), "H6200", b.home_checking, 200_00, None);

    let early = b.s.month_budget("2026-03", d(3, 3)).unwrap();
    assert_eq!(early.lines[0].progress, Cents(200_00));
    assert_eq!(early.lines[0].status.zone, Zone::TrendingOver);
    assert_eq!(early.lines[0].percent_used, Some(50));
    assert_eq!(b.s.month_budget("2026-03", d(3, 16)).unwrap().lines[0].status.zone, Zone::InRange);
    // Judged from the outside, a finished month is judged at its last day.
    assert_eq!(b.s.month_budget("2026-03", d(9, 1)).unwrap().lines[0].status.zone, Zone::Under);

    // A refund comes off; a voided purchase never happened.
    post(&mut b.s, d(3, 4), "refund", b.home_checking, groceries, 20_00);
    let oops = spend(&mut b.s, d(3, 5), "H6200", b.home_checking, 999_00, None);
    b.s.void_entry(oops, d(4, 2)).unwrap();
    let m = b.s.month_budget("2026-03", d(3, 16)).unwrap();
    assert_eq!(m.lines[0].progress, Cents(180_00));
    assert_eq!(m.spending, Cents(180_00));
}

#[test]
fn people_budgets_count_what_was_for_them() {
    let mut b = both();
    let mom = b.s.create_person(&NewPerson { name: "Mom".into(), color: None }).unwrap();
    let ethan = b.s.create_person(&NewPerson { name: "Ethan".into(), color: Some("teal".into()) }).unwrap();
    b.s.set_budget_line("2026-03", &ceiling(BudgetTarget::Person(ethan), 150_00)).unwrap();
    spend(&mut b.s, d(3, 3), "H6510", b.home_checking, 90_00, Some(ethan));
    spend(&mut b.s, d(3, 4), "H6700", b.home_checking, 80_00, Some(ethan));
    spend(&mut b.s, d(3, 5), "H6710", b.home_checking, 40_00, Some(mom));
    spend(&mut b.s, d(3, 6), "H6200", b.home_checking, 300_00, None);

    let m = b.s.month_budget("2026-03", d(3, 31)).unwrap();
    let line = &m.lines[0];
    assert_eq!((line.name.as_str(), line.progress), ("Ethan", Cents(170_00)));
    assert_eq!((line.status.zone, line.status.sentiment), (Zone::Over, Sentiment::Alarm));
    assert_eq!(m.planned_for_people, Cents(150_00));
    let of = |p: Option<i64>| m.people.iter().find(|x| x.person_id == p).unwrap().spending;
    assert_eq!((of(Some(mom)), of(Some(ethan)), of(None)), (Cents(40_00), Cents(170_00), Cents(300_00)));
    assert_eq!(m.categories[0].name, "Groceries");

    // A person's budget is a limit; floors are for savings and income.
    let floor = BudgetLine { direction: Direction::Floor, ..ceiling(BudgetTarget::Person(mom), 10_00) };
    assert!(b.s.set_budget_line("2026-03", &floor).is_err());
    // Business categories aren't budgeted here.
    assert!(b.s.set_budget_line("2026-03", &ceiling(BudgetTarget::Account(code(&b.s, "6500")), 10_00)).is_err());

    // Archived people drop out of later copies of the plan.
    b.s.set_person_archived(ethan, true).unwrap();
    b.s.set_available("2026-04", Some(Cents(100_00))).unwrap();
    assert!(b.s.month_budget("2026-04", d(4, 1)).unwrap().lines.is_empty());
}

#[test]
fn savings_and_income_goals_are_floors() {
    let mut b = both();
    let savings = b.s.create_account(&NewAccount::new("Savings", AccountSubtype::Bank).scope(Scope::Household)).unwrap();
    let floor = |t, c| BudgetLine { target: t, amount: Cents(c), direction: Direction::Floor, tolerance_pct: 10 };
    b.s.set_budget_line("2026-03", &floor(BudgetTarget::Account(savings), 200_00)).unwrap();
    b.s.set_budget_line("2026-03", &floor(BudgetTarget::Account(code(&b.s, "H4200")), 1500_00)).unwrap();
    post(&mut b.s, d(3, 1), "to savings", savings, b.home_checking, 50_00);
    let side = code(&b.s, "H4200");
    post(&mut b.s, d(3, 2), "Freelance", b.home_checking, side, 1600_00);

    let m = b.s.month_budget("2026-03", d(3, 31)).unwrap();
    let saved = m.lines.iter().find(|l| l.target == BudgetTarget::Account(savings)).unwrap();
    assert_eq!((saved.progress, saved.status.sentiment), (Cents(50_00), Sentiment::Alarm));
    let earned = m.lines.iter().find(|l| l.name == "Side Income").unwrap();
    assert_eq!((earned.status.zone, earned.status.sentiment), (Zone::Over, Sentiment::Good));
    assert_eq!(m.income, Cents(1600_00));
    assert_eq!(m.planned_for_categories, Cents::ZERO, "floors aren't spending");
}

#[test]
fn rules_know_who_it_was_for() {
    use tally_core::bank::{FeedBatch, FeedTransaction};
    use tally_core::rules::{Direction as RuleDirection, MatchOp};
    use tally_store::NewRule;

    let mut b = both();
    let ethan = b.s.create_person(&NewPerson { name: "Ethan".into(), color: None }).unwrap();
    let fun = code(&b.s, "H6740");
    b.s.create_rule(&NewRule {
        name: "Roblox".into(),
        priority: 100,
        op: MatchOp::Contains,
        pattern: "roblox".into(),
        direction: RuleDirection::Outflow,
        amount_min: None,
        amount_max: None,
        bank_account_id: None,
        target_account_id: fun,
        rename_to: None,
        auto_post: true,
        person_id: Some(ethan),
    })
    .unwrap();
    let txn = |id: &str, desc: &str, cents: i64| FeedTransaction {
        external_id: id.into(),
        date: d(3, 3),
        amount: Cents(cents),
        description: desc.into(),
        merchant: None,
        category_hint: None,
        pending: false,
    };
    let batch = FeedBatch { added: vec![txn("1", "ROBLOX CORP", -9_99), txn("2", "SUPERCUTS 123", -25_00)], ..Default::default() };
    let summary = b.s.import_feed(b.home_checking, &batch).unwrap();
    assert_eq!(summary.auto_posted, 1);
    assert_eq!(b.s.month_budget("2026-03", d(3, 31)).unwrap().people.iter().find(|p| p.person_id == Some(ethan)).unwrap().spending, Cents(9_99));

    // The haircut waits for review; once tagged, the next one is suggested for the same person.
    let cut = b.s.inbox(None).unwrap().into_iter().next().unwrap();
    assert_eq!(cut.person_id, None);
    let entry = b.s.accept(cut.txn.id, code(&b.s, "H6710"), None).unwrap();
    b.s.set_entry_person(entry, Some(ethan)).unwrap();
    b.s.import_feed(b.home_checking, &FeedBatch { added: vec![txn("3", "SUPERCUTS 123", -25_00)], ..Default::default() }).unwrap();
    assert_eq!(b.s.inbox(None).unwrap()[0].person_id, Some(ethan));
}

#[test]
fn rent_on_the_first_doesnt_look_like_overspending() {
    let mut b = both();
    let rent = code(&b.s, "H6000");
    b.s.set_budget_line("2026-03", &ceiling(BudgetTarget::Account(rent), 1495_00)).unwrap();
    b.s.set_available("2026-03", Some(Cents(3200_00))).unwrap();
    spend(&mut b.s, d(3, 1), "H6000", b.home_checking, 1495_00, None);
    spend(&mut b.s, d(3, 10), "H6200", b.home_checking, 500_00, None);
    let m = b.s.month_budget("2026-03", d(3, 14)).unwrap();
    assert_eq!(m.lines[0].status.zone, Zone::InRange, "paid in full on its day is on plan");
    let overall = m.overall.unwrap();
    assert_eq!(overall.pace, Cents(1495_00 + 1107_14), "$500 by day 14 of 31 projects to $1,107.14");
    assert_eq!(overall.zone, Zone::Under);
}

// ---------------------------------------------------------------- recurring bills

use tally_core::schedule::Cadence;
use tally_store::{DueState, NewRecurringBill};

fn bill(name: &str, category: AccountId, cents: i64, day: u32) -> NewRecurringBill {
    NewRecurringBill {
        name: name.into(),
        amount: Cents(cents),
        account_id: category,
        paid_from_id: None,
        person_id: None,
        cadence: Cadence::Month { day },
        start_date: d(1, 1),
        auto_pay: false,
        match_text: None,
    }
}

#[test]
fn a_payment_near_the_due_date_marks_it_paid() {
    let mut b = both();
    let phones = code(&b.s, "H6130");
    let verizon = b.s.create_recurring_bill(&NewRecurringBill { match_text: Some("verizon".into()), ..bill("Verizon", phones, 160_00, 14) }).unwrap();
    b.s.create_recurring_bill(&bill("Rent", code(&b.s, "H6000"), 1495_00, 1)).unwrap();

    // A different phone charge doesn't count; Verizon's, two days late and a bit more, does.
    let other = code(&b.s, "H6130");
    b.s.post_entry(&NewEntry::new(d(3, 14), "Mint Mobile").line(EntryLine::debit(other, Cents(160_00))).line(EntryLine::credit(b.home_checking, Cents(160_00)))).unwrap();
    let paid = b.s.post_entry(&NewEntry::new(d(3, 16), "VERIZON WIRELESS").line(EntryLine::debit(phones, Cents(162_53))).line(EntryLine::credit(b.home_checking, Cents(162_53)))).unwrap();

    let due = b.s.due_dates(d(3, 1), d(3, 31), d(3, 20)).unwrap();
    let rent = due.iter().find(|x| x.name == "Rent").unwrap();
    assert_eq!(rent.state, DueState::Overdue);
    let v = due.iter().find(|x| x.bill_id == verizon).unwrap();
    assert_eq!(v.state, DueState::Paid);
    assert_eq!(v.payment.as_ref().unwrap().entry_id, paid);

    // April's isn't paid by March's payment.
    let april = b.s.due_dates(d(4, 1), d(4, 30), d(4, 10)).unwrap();
    assert_eq!(april.iter().find(|x| x.bill_id == verizon).unwrap().state, DueState::Upcoming);
}

#[test]
fn states_today_auto_pay_and_marks() {
    let b = both();
    let rent = code(&b.s, "H6000");
    let internet = b.s.create_recurring_bill(&NewRecurringBill { auto_pay: true, ..bill("Internet", code(&b.s, "H6120"), 65_00, 17) }).unwrap();
    let rent_bill = b.s.create_recurring_bill(&bill("Rent", rent, 1495_00, 1)).unwrap();
    let state = |s: &Store, id, today| s.due_dates(d(3, 1), d(3, 31), today).unwrap().into_iter().find(|x| x.bill_id == id).unwrap().state;

    assert_eq!(state(&b.s, internet, d(3, 17)), DueState::DueToday);
    assert_eq!(state(&b.s, internet, d(3, 19)), DueState::Processing, "auto-pay gets a few days' grace");
    assert_eq!(state(&b.s, internet, d(3, 25)), DueState::Overdue);

    // Paid in cash, outside the books: mark it by hand. Then undo.
    b.s.mark_due_date(rent_bill, d(3, 1), Some(DueState::Paid), None).unwrap();
    assert_eq!(state(&b.s, rent_bill, d(3, 20)), DueState::Paid);
    b.s.mark_due_date(rent_bill, d(3, 1), Some(DueState::Skipped), None).unwrap();
    assert_eq!(state(&b.s, rent_bill, d(3, 20)), DueState::Skipped);
    b.s.mark_due_date(rent_bill, d(3, 1), None, None).unwrap();
    assert_eq!(state(&b.s, rent_bill, d(3, 20)), DueState::Overdue);
    assert!(b.s.mark_due_date(rent_bill, d(3, 2), Some(DueState::Paid), None).is_err(), "not a due date");

    b.s.set_recurring_bill_active(internet, false).unwrap();
    assert!(b.s.due_dates(d(3, 1), d(3, 31), d(3, 1)).unwrap().iter().all(|x| x.bill_id != internet));
}

#[test]
fn bills_still_to_come_count_in_the_projection() {
    let mut b = both();
    let utilities = code(&b.s, "H6100");
    b.s.create_recurring_bill(&bill("PG&E", utilities, 150_00, 25)).unwrap();
    b.s.set_available("2026-03", Some(Cents(1000_00))).unwrap();
    spend(&mut b.s, d(3, 5), "H6200", b.home_checking, 300_00, None);
    let m = b.s.month_budget("2026-03", d(3, 15)).unwrap();
    assert_eq!(m.bills_due, Cents(150_00));
    // $300 by day 15 of 31 is $620 at that pace, plus the $150 bill still to come.
    assert_eq!(m.overall.unwrap().pace, Cents(620_00 + 150_00));
}

#[test]
fn repeat_payments_are_suggested_as_bills() {
    let mut b = both();
    let phones = code(&b.s, "H6130");
    let groceries = code(&b.s, "H6200");
    for (m, day, cents) in [(6, 11, 160_88), (7, 12, 160_88), (8, 11, 158_20), (9, 10, 160_88)] {
        post(&mut b.s, d(m, day), "Verizon", phones, b.home_checking, cents);
        post(&mut b.s, d(m, day * 2), "Safeway", groceries, b.home_checking, 50_00 * i64::from(m));
    }
    let s = b.s.bill_suggestions(d(10, 5)).unwrap();
    assert_eq!(s.len(), 1, "groceries vary too much to be a bill: {s:?}");
    assert_eq!((s[0].name.as_str(), s[0].account_id, s[0].amount, s[0].months_seen), ("Verizon", phones, Cents(160_88), 4));

    b.s.create_recurring_bill(&NewRecurringBill { match_text: Some(s[0].match_text.clone()), ..bill("Verizon", phones, 160_88, 11) }).unwrap();
    assert!(b.s.bill_suggestions(d(10, 5)).unwrap().is_empty(), "already a bill");
}
