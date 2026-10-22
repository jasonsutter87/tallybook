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
        varies: false,
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
        // A couple of grocery runs a month isn't a schedule.
        post(&mut b.s, d(m, day - 7), "Safeway", groceries, b.home_checking, 50_00 * i64::from(m));
        post(&mut b.s, d(m, day + 9), "Safeway", groceries, b.home_checking, 30_00 * i64::from(m));
    }
    let s = b.s.bill_suggestions(d(10, 5)).unwrap();
    assert_eq!(s.len(), 1, "groceries aren't a bill: {s:?}");
    assert_eq!((s[0].name.as_str(), s[0].account_id, s[0].amount, s[0].times_seen), ("Verizon", Some(phones), Cents(160_88), 4));
    assert_eq!((s[0].cadence, s[0].start_date, s[0].in_review), (Cadence::Month { day: 11 }, d(6, 11), 0));

    b.s.create_recurring_bill(&NewRecurringBill { match_text: Some(s[0].match_text.clone()), ..bill("Verizon", phones, 160_88, 11) }).unwrap();
    assert!(b.s.bill_suggestions(d(10, 5)).unwrap().is_empty(), "already a bill");
}

#[test]
fn a_bill_named_like_a_payee_only_covers_it_when_the_amount_fits() {
    let mut b = both();
    let fuel = code(&b.s, "H6200");
    for m in 6..=9 {
        post(&mut b.s, d(m, 8), "SHELL GAS 4411", fuel, b.home_checking, 45_00);
    }
    assert_eq!(b.s.bill_suggestions(d(10, 5)).unwrap()[0].name, "Shell Gas");
    // A $200 "Gas" utility bill isn't this $45 fill-up...
    b.s.create_recurring_bill(&bill("Gas", fuel, 200_00, 20)).unwrap();
    assert_eq!(b.s.bill_suggestions(d(10, 5)).unwrap().len(), 1);
    // ...but a $45 one is.
    b.s.create_recurring_bill(&bill("Gas", fuel, 45_00, 8)).unwrap();
    assert!(b.s.bill_suggestions(d(10, 5)).unwrap().is_empty());
}

#[test]
fn a_fresh_bank_import_is_enough_to_spot_bills() {
    use tally_core::bank::{FeedBatch, FeedTransaction};
    let mut b = both();
    let txn = |id: String, date: NaiveDate, desc: &str, cents: i64| FeedTransaction {
        external_id: id,
        date,
        amount: Cents(cents),
        description: desc.into(),
        merchant: None,
        category_hint: None,
        pending: false,
    };
    let mut added = Vec::new();
    // May through September. Power moves with the weather; Netflix doesn't;
    // the card gets paid off; the gym was quit in June.
    let power_bills = [118_40, 162_10, 188_75, 171_30, 131_05];
    for (m, power) in (5..=9).zip(power_bills) {
        added.push(txn(format!("pge{m}"), d(m, 18), "PGE WEB ONLINE 0518", -power));
        added.push(txn(format!("nfx{m}"), d(m, 2), "NETFLIX.COM 866", -15_49));
        added.push(txn(format!("card{m}"), d(m, 25), "ONLINE TRANSFER TO CARD 4411", -400_00));
        if m <= 6 {
            added.push(txn(format!("gym{m}"), d(m, 9), "PLANET FITNESS", -24_99));
        }
    }
    // A weekly allowance by Venmo.
    for w in 0..4 {
        added.push(txn(format!("ven{w}"), d(9, 4) + chrono::Days::new(w * 7), "VENMO *ETHAN", -10_00));
    }
    b.s.import_feed(b.home_checking, &FeedBatch { added, ..Default::default() }).unwrap();

    let s = b.s.bill_suggestions(d(10, 1)).unwrap();
    let names: Vec<&str> = s.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, ["Pge Web Online", "Netflix.com", "Venmo Ethan"], "biggest first, nothing stale or transferred");
    let power = &s[0];
    assert!(power.varies);
    assert_eq!((power.amount, power.cadence, power.in_review, power.account_id, power.paid_from_id), (Cents(162_10), Cadence::Month { day: 18 }, 5, None, Some(b.home_checking)));
    assert_eq!(s[2].cadence, Cadence::Week);

    // "Not a bill" sticks.
    b.s.dismiss_bill_suggestion(&s[2].match_text).unwrap();
    assert_eq!(b.s.bill_suggestions(d(10, 1)).unwrap().len(), 2);
}

#[test]
fn hostile_bank_text_and_amounts_dont_break_suggestions() {
    use tally_core::bank::{FeedBatch, FeedTransaction};
    let mut b = both();
    let txn = |id: String, date: NaiveDate, desc: String, cents: i64| FeedTransaction {
        external_id: id,
        date,
        amount: Cents(cents),
        description: desc,
        merchant: None,
        category_hint: None,
        pending: false,
    };
    let huge = format!("ÉLECTRICITÉ 日本 {}", "Z".repeat(50_000));
    let mut added = Vec::new();
    for m in 6..=9 {
        // Amounts at the edges of i64; i64::MIN has no positive and is skipped.
        added.push(txn(format!("max{m}"), d(m, 3), "HUGE AMOUNT CO".into(), -i64::MAX));
        added.push(txn(format!("min{m}"), d(m, 4), "MIN AMOUNT CO".into(), i64::MIN));
        added.push(txn(format!("big{m}"), d(m, 5), huge.clone(), -12_00));
    }
    b.s.import_feed(b.home_checking, &FeedBatch { added, ..Default::default() }).unwrap();

    let s = b.s.bill_suggestions(d(10, 1)).unwrap();
    let names: Vec<&str> = s.iter().map(|x| x.name.as_str()).collect();
    // An i64::MAX bill is no household bill (`MAX_BILL_AMOUNT`).
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(names[0].starts_with("Électricité 日本 Z") && names[0].chars().count() <= 80);
    assert!(s[0].match_text.chars().count() <= 120);

    // The capped key round-trips through "Not a bill", even sent in full.
    b.s.dismiss_bill_suggestion(&huge).unwrap();
    assert!(b.s.bill_suggestions(d(10, 1)).unwrap().is_empty());
}

#[test]
fn dismissals_are_bounded_and_waving_off_always_works() {
    let b = both();
    for junk in ["", "  ", "##", "12345", "ab"] {
        assert!(matches!(b.s.dismiss_bill_suggestion(junk), Err(tally_store::StoreError::Rule(_))), "{junk:?}");
    }
    for i in 0..1000 {
        b.s.dismiss_bill_suggestion(&format!("payee {}", char::from(b'a' + (i % 26) as u8).to_string().repeat(i / 26 + 1))).unwrap();
    }
    // Full: the oldest is forgotten rather than "Not a bill" failing.
    b.s.dismiss_bill_suggestion("one too many").unwrap();
    // Waving off one already on the list is still fine.
    b.s.dismiss_bill_suggestion("PAYEE A").unwrap();
}

// Bank text and amounts are whatever the bank, or whoever wrote the
// description, says. These are the Black Team's attacks on bill detection,
// kept as regressions.

fn feed_txn(id: String, date: NaiveDate, desc: &str, merchant: Option<&str>, cents: i64) -> tally_core::bank::FeedTransaction {
    tally_core::bank::FeedTransaction {
        external_id: id,
        date,
        amount: Cents(cents),
        description: desc.into(),
        merchant: merchant.map(Into::into),
        category_hint: None,
        pending: false,
    }
}

fn import(b: &mut Both, added: Vec<tally_core::bank::FeedTransaction>) {
    b.s.import_feed(b.home_checking, &tally_core::bank::FeedBatch { added, ..Default::default() }).unwrap();
}

/// What "Add as a bill" in BillSuggestions.tsx sends, once a category is picked.
fn bill_from(s: &tally_store::BillSuggestion, category: AccountId) -> NewRecurringBill {
    NewRecurringBill {
        name: s.name.clone(),
        amount: s.amount,
        varies: s.varies,
        account_id: s.account_id.unwrap_or(category),
        paid_from_id: s.paid_from_id,
        person_id: s.person_id,
        cadence: s.cadence,
        start_date: s.start_date,
        auto_pay: false,
        match_text: Some(s.match_text.clone()),
    }
}

#[test]
fn a_different_ach_originator_behind_the_same_text_is_a_different_payee() {
    let mut b = both();
    let insurance = code(&b.s, "H6010");
    for m in 6..=9 {
        post(&mut b.s, d(m, 5), "ACH DEBIT 8005551212", insurance, b.home_checking, 50_00);
    }
    let s = b.s.bill_suggestions(d(10, 1)).unwrap();
    assert_eq!(s[0].match_text, "ach debit 8005551212", "the originator's id is part of the payee");
    b.s.create_recurring_bill(&bill_from(&s[0], insurance)).unwrap();

    // Someone else debiting under the same bank text, monthly on the 20th.
    for m in 6..=10 {
        post(&mut b.s, d(m, 20), "ACH DEBIT 3334445555", insurance, b.home_checking, 49_00);
    }
    let s = b.s.bill_suggestions(d(10, 21)).unwrap();
    assert_eq!(s.iter().map(|x| x.match_text.as_str()).collect::<Vec<_>>(), ["ach debit 3334445555"], "surfaced, not hidden");
    // And it can't pay the real bill.
    post(&mut b.s, d(10, 28), "ACH DEBIT 3334445555", insurance, b.home_checking, 49_00);
    let due = &b.s.due_dates(d(11, 1), d(11, 30), d(11, 6)).unwrap()[0];
    assert_eq!((due.state, due.payment.is_none()), (DueState::Overdue, true));
    post(&mut b.s, d(11, 5), "ACH DEBIT 8005551212", insurance, b.home_checking, 50_00);
    assert_eq!(b.s.due_dates(d(11, 1), d(11, 30), d(11, 6)).unwrap()[0].state, DueState::Paid);
}

#[test]
fn store_numbers_and_reference_numbers_still_group() {
    let mut b = both();
    let (groceries, internet) = (code(&b.s, "H6200"), code(&b.s, "H6120"));
    for m in 6..=9 {
        let store = if m % 2 == 0 { "SAFEWAY #1234" } else { "SAFEWAY #5678" };
        post(&mut b.s, d(m, 3), store, groceries, b.home_checking, 80_00);
        // A fresh reference number on every payment.
        post(&mut b.s, d(m, 9), &format!("COMCAST PAYMENT REF 99812{m:05}"), internet, b.home_checking, 65_00);
    }
    let s = b.s.bill_suggestions(d(10, 1)).unwrap();
    let keys: Vec<&str> = s.iter().map(|x| x.match_text.as_str()).collect();
    assert_eq!(keys, ["safeway", "comcast payment ref"]);
}

#[test]
fn a_look_alike_isnt_hidden_by_a_bill_it_merely_contains() {
    let mut b = both();
    let tv = code(&b.s, "H6120");
    let netflix = NewRecurringBill { match_text: Some("NETFLIX.COM".into()), cadence: Cadence::Month { day: 2 }, ..bill("Netflix", tv, 15_49, 2) };
    b.s.create_recurring_bill(&netflix).unwrap();
    for m in 6..=9 {
        // The real thing is covered...
        post(&mut b.s, d(m, 2), "NETFLIX.COM 866", tv, b.home_checking, 15_49);
        // ...a charge five times the size that merely contains it isn't.
        post(&mut b.s, d(m, 14), "NETFLIX.COM-PREMIUM-BILLING-SVC", tv, b.home_checking, 89_99);
    }
    let s = b.s.bill_suggestions(d(10, 1)).unwrap();
    assert_eq!(s.iter().map(|x| x.match_text.as_str()).collect::<Vec<_>>(), ["netflix.com-premium-billing-svc"]);

    // A short match text covers whole words only.
    b.s.create_recurring_bill(&NewRecurringBill { match_text: Some("pos".into()), ..bill("Gym", tv, 300_00, 22) }).unwrap();
    for m in 6..=9 {
        post(&mut b.s, d(m, 22), "POSSIBLE SECURITY SVC", tv, b.home_checking, 300_00);
    }
    assert!(b.s.bill_suggestions(d(10, 1)).unwrap().iter().any(|x| x.match_text == "possible security svc"));
}

#[test]
fn a_usage_bill_keeps_its_typical_amount_and_wont_take_just_any_charge() {
    let mut b = both();
    let power = code(&b.s, "H6100");
    import(&mut b, (5..=9).zip([118_40, 162_10, 188_75, 171_30, 131_05]).map(|(m, c)| feed_txn(format!("p{m}"), d(m, 18), "PGE WEB ONLINE 0518", None, -c)).collect());
    let s = b.s.bill_suggestions(d(10, 1)).unwrap();
    assert!(s[0].varies);
    let bill = bill_from(&s[0], power);
    assert_eq!((bill.amount, bill.varies), (Cents(162_10), true));
    b.s.create_recurring_bill(&bill).unwrap();

    // $4,999 under junk text, with the payee's name in the merchant field...
    import(&mut b, vec![feed_txn("evil".into(), d(10, 16), "QZX HOLDINGS LLC", Some("PGE WEB ONLINE"), -4_999_00)]);
    let accept = |b: &mut Both, ext: &str| {
        let id = b.s.inbox(None).unwrap().into_iter().find(|i| i.txn.external_id == ext).unwrap().txn.id;
        b.s.accept(id, power, None).unwrap();
    };
    accept(&mut b, "evil");
    let due = &b.s.due_dates(d(10, 1), d(10, 31), d(10, 20)).unwrap()[0];
    assert_eq!((due.state, due.payment.is_none()), (DueState::Overdue, true), "isn't a power bill");
    // ...but a cold month's real bill is.
    import(&mut b, vec![feed_txn("oct".into(), d(10, 19), "PGE WEB ONLINE 1018", None, -241_60)]);
    accept(&mut b, "oct");
    let due = &b.s.due_dates(d(10, 1), d(10, 31), d(10, 20)).unwrap()[0];
    assert_eq!((due.state, due.payment.as_ref().map(|p| p.amount)), (DueState::Paid, Some(Cents(241_60))));

    // A bill that varies needs its typical amount.
    assert!(matches!(b.s.create_recurring_bill(&NewRecurringBill { amount: Cents::ZERO, ..bill }), Err(tally_store::StoreError::Rule(_))));
}

#[test]
fn suggested_names_hide_invisible_characters_and_look_alikes() {
    let mut b = both();
    let mut added = vec![];
    for m in 6..=9 {
        // A merchant field that names someone else, drawn backwards with a bidi override.
        added.push(feed_txn(format!("a{m}"), d(m, 3), "QZX HOLDINGS LLC", Some("\u{202E}xilfteN\u{200B} \u{202C}"), -15_49));
        // A Cyrillic Е.
        added.push(feed_txn(format!("b{m}"), d(m, 4), "NЕTFLIX.COM", None, -17_99));
        // A merchant name that is the payee is still the nicer name.
        added.push(feed_txn(format!("c{m}"), d(m, 5), "SPOTIFY USA 8775", Some("Spotify"), -11_99));
    }
    import(&mut b, added);
    let s = b.s.bill_suggestions(d(10, 1)).unwrap();
    let names: Vec<(&str, &str)> = s.iter().map(|x| (x.name.as_str(), x.match_text.as_str())).collect();
    assert_eq!(names, [("N?tflix.com", "nеtflix.com"), ("Qzx Holdings Llc", "qzx holdings llc"), ("Spotify", "spotify usa")]);
}

#[test]
fn a_flood_of_made_up_payees_is_capped_and_can_be_waved_off() {
    let mut b = both();
    let name = |i: usize| {
        let mut n = String::from("PAYEE ");
        let mut k = i;
        loop {
            n.push(char::from(b'a' + (k % 26) as u8));
            k /= 26;
            if k == 0 {
                break;
            }
        }
        n
    };
    let added = (0..1005).flat_map(|i| (7..=9).map(move |m| (i, m))).map(|(i, m)| feed_txn(format!("{i}-{m}"), d(m, 10), &name(i), None, -9_99)).collect();
    import(&mut b, added);
    assert_eq!(b.s.bill_suggestions(d(10, 1)).unwrap().len(), 50, "shown at most");
    for i in 0..1005 {
        b.s.dismiss_bill_suggestion(&name(i)).unwrap();
    }
    // The five waved off first were forgotten to make room.
    let left: Vec<String> = b.s.bill_suggestions(d(10, 1)).unwrap().into_iter().map(|x| x.match_text).collect();
    assert_eq!(left.len(), 5, "{left:?}");
}

#[test]
fn an_absurd_amount_never_becomes_a_bill_or_breaks_the_budget() {
    let mut b = both();
    let cat = code(&b.s, "H6210");
    let amt = 2_400_000_000_000_000_000_i64;
    import(&mut b, (0..5).map(|w| feed_txn(format!("w{w}"), d(9, 1) + chrono::Days::new(w * 7), "WEEKLY THING", None, -amt)).collect());
    assert!(b.s.bill_suggestions(d(10, 1)).unwrap().is_empty(), "no household bill is that big");
    let weekly = NewRecurringBill { cadence: Cadence::Week, start_date: d(9, 1), ..bill("Weekly thing", cat, amt, 1) };
    assert!(matches!(b.s.create_recurring_bill(&weekly), Err(tally_store::StoreError::Rule(_))));
    // The biggest allowed, weekly, sums fine.
    b.s.create_recurring_bill(&NewRecurringBill { amount: tally_store::MAX_BILL_AMOUNT, ..weekly }).unwrap();
    let m = b.s.month_budget("2026-10", d(10, 1)).unwrap();
    assert!(m.bills_due >= tally_store::MAX_BILL_AMOUNT);
}

/// Bill detection with a huge Review backlog. `cargo test --release -- --ignored --nocapture` to time it.
#[test]
#[ignore]
fn suggestions_with_a_huge_review_backlog() {
    for n in [10_000usize, 50_000, 100_000] {
        let mut b = both();
        let added = (0..n)
            .map(|i| {
                let desc = format!("MERCHANT {}", (b'a' + (i % 26) as u8) as char);
                feed_txn(format!("x{i}"), d(10, 1) - chrono::Days::new((i % 700) as u64), &desc, None, -1_00 - (i as i64 % 5000))
            })
            .collect();
        import(&mut b, added);
        let t = std::time::Instant::now();
        let k = b.s.bill_suggestions(d(10, 1)).unwrap().len();
        let took = t.elapsed();
        println!("n={n} suggestions({k}) in {took:?}");
        assert!(took < std::time::Duration::from_secs(2));
    }
}

// ---------------------------------------------------------------- goals & insights

use tally_core::insights::Band;
use tally_store::{NewGoal, Tone};

fn savings(b: &Both) -> AccountId {
    b.s.create_account(&NewAccount::new("OnPoint Savings", AccountSubtype::Bank).scope(Scope::Household)).unwrap()
}

#[test]
fn goals_fill_from_tagged_transfers_and_say_when_theyre_done() {
    let mut b = both();
    let savings = savings(&b);
    let hawaii = b
        .s
        .create_goal(&NewGoal { name: "Hawaii".into(), target: Cents(1000_00), target_date: Some(d(12, 1)), account_id: savings, starting: Cents(100_00), note: String::new() })
        .unwrap();
    assert!(!b.s.contribute_to_goal(hawaii, b.home_checking, Cents(400_00), d(3, 5)).unwrap().finished);
    // An untagged transfer to the same savings account isn't counted...
    let other = post(&mut b.s, d(4, 5), "to savings", savings, b.home_checking, 50_00);
    let g = b.s.goals(d(4, 10)).unwrap().remove(0);
    assert_eq!((g.saved, g.contributions), (Cents(500_00), 1));
    assert_eq!(g.projection.monthly_rate, Cents(400_00), "one month of contributions; the starting $100 isn't a pace");
    // ...until it's tagged.
    b.s.set_entry_goal(other, Some(hawaii)).unwrap();
    assert_eq!(b.s.goals(d(4, 10)).unwrap()[0].saved, Cents(550_00));

    let done = b.s.contribute_to_goal(hawaii, b.home_checking, Cents(450_00), d(5, 5)).unwrap();
    assert!(done.finished, "this one reached the target");
    assert!(b.s.goals(d(5, 10)).unwrap()[0].achieved_at.is_some());
    // Taking money back out un-finishes it.
    b.s.contribute_to_goal(hawaii, b.home_checking, Cents(-100_00), d(5, 6)).unwrap();
    assert!(b.s.goals(d(5, 10)).unwrap()[0].achieved_at.is_none());

    // Goals keep their money somewhere real and household.
    let biz = NewGoal { name: "x".into(), target: Cents(1), target_date: None, account_id: b.biz_checking, starting: Cents::ZERO, note: String::new() };
    assert!(b.s.create_goal(&biz).is_err());
    let dining = code(&b.s, "H6210");
    let lunch = post(&mut b.s, d(5, 7), "lunch", dining, b.home_checking, 9_00);
    assert!(b.s.set_entry_goal(lunch, Some(hawaii)).is_err(), "lunch never touched the savings account");
}

#[test]
fn alerts_at_eighty_percent_over_and_for_unpaid_bills() {
    let mut b = both();
    let groceries = code(&b.s, "H6200");
    let dining = code(&b.s, "H6210");
    b.s.set_budget_line("2026-03", &ceiling(BudgetTarget::Account(groceries), 1000_00)).unwrap();
    b.s.set_budget_line("2026-03", &ceiling(BudgetTarget::Account(dining), 100_00)).unwrap();
    spend(&mut b.s, d(3, 2), "H6200", b.home_checking, 850_00, None);
    spend(&mut b.s, d(3, 3), "H6210", b.home_checking, 120_00, None);
    b.s.create_recurring_bill(&bill("Verizon", code(&b.s, "H6130"), 160_00, 11)).unwrap();

    let alerts = b.s.alerts("2026-03", d(3, 25)).unwrap();
    let titles: Vec<&str> = alerts.iter().map(|a| a.title.as_str()).collect();
    assert_eq!(alerts[0].tone, Tone::Alarm);
    assert!(titles.contains(&"Dining Out is over budget"), "{titles:?}");
    assert!(titles.contains(&"You've used 85% of your Groceries budget"), "{titles:?}");
    assert!(titles.contains(&"Verizon looks unpaid"), "{titles:?}");
}

#[test]
fn opportunity_gaps_split_spending_by_how_far_it_bends() {
    let mut b = both();
    for m in 1..=3 {
        spend(&mut b.s, d(m, 5), "H6210", b.home_checking, 300_00, None);
    }
    spend(&mut b.s, d(4, 1), "H6000", b.home_checking, 1495_00, None);
    spend(&mut b.s, d(4, 3), "H6200", b.home_checking, 600_00, None);
    spend(&mut b.s, d(4, 5), "H6210", b.home_checking, 500_00, None);
    let child = code(&b.s, "H4100");
    post(&mut b.s, d(4, 1), "support", b.home_checking, child, 2000_00);

    let o = b.s.opportunities("2026-04").unwrap();
    assert_eq!((o.income, o.spending, o.gap), (Cents(2000_00), Cents(2595_00), Cents(-595_00)));
    let tier = |f| o.tiers.iter().find(|t| t.flex == f).unwrap();
    assert_eq!(tier(Some(Flex::Fixed)).spending, Cents(1495_00));
    assert_eq!(tier(Some(Flex::Lifestyle)).spending, Cents(500_00));
    assert_eq!(tier(Some(Flex::Lifestyle)).usual, Cents(300_00));
    assert_eq!(o.lifestyle_fifth, Cents(100_00));
    assert_eq!(o.categories[0].name, "Groceries");
    assert!(o.categories.iter().all(|c| c.name != "Rent or Mortgage"), "fixed costs aren't opportunities");
}

#[test]
fn the_score_rewards_living_within_income_and_paying_bills() {
    let mut b = both();
    let support = code(&b.s, "H4100");
    for m in 1..=6 {
        post(&mut b.s, d(m, 1), "support", b.home_checking, support, 4000_00);
        spend(&mut b.s, d(m, 3), "H6200", b.home_checking, 3000_00, None);
    }
    let good = b.s.budget_score("2026-06", d(6, 30)).unwrap();
    assert!(good.score >= 670, "{good:?}");
    assert!(matches!(good.band, Band::Good | Band::VeryGood | Band::Excellent));

    b.s.create_recurring_bill(&bill("Rent", code(&b.s, "H6000"), 1495_00, 1)).unwrap();
    let missed = b.s.budget_score("2026-06", d(6, 30)).unwrap();
    assert!(missed.score < good.score, "an unpaid bill costs points");
    assert_eq!(missed.factors.iter().find(|f| f.key == "bills").unwrap().summary, "0 of 1 due so far are paid");
}

#[test]
fn wins_are_true_and_specific() {
    let mut b = both();
    let savings = savings(&b);
    let dining = code(&b.s, "H6210");
    let support = code(&b.s, "H4100");
    b.s.set_budget_line("2026-02", &ceiling(BudgetTarget::Account(dining), 200_00)).unwrap();
    for m in 1..=2 {
        post(&mut b.s, d(m, 1), "support", b.home_checking, support, 3000_00);
    }
    spend(&mut b.s, d(1, 5), "H6210", b.home_checking, 400_00, None);
    spend(&mut b.s, d(2, 5), "H6210", b.home_checking, 150_00, None);
    let hawaii = b.s.create_goal(&NewGoal { name: "Hawaii".into(), target: Cents(100_00), target_date: None, account_id: savings, starting: Cents::ZERO, note: String::new() }).unwrap();
    b.s.contribute_to_goal(hawaii, b.home_checking, Cents(100_00), d(3, 2)).unwrap();
    let side = code(&b.s, "H4200");
    post(&mut b.s, d(3, 4), "Etsy", b.home_checking, side, 250_00);

    let wins = b.s.wins("2026-03", d(3, 10)).unwrap();
    let titles: Vec<&str> = wins.iter().map(|w| w.title.as_str()).collect();
    assert!(titles.contains(&"$50 under on Dining Out"), "{titles:?}");
    assert!(titles.contains(&"February ended $2,850 ahead"), "{titles:?}");
    assert!(titles.contains(&"$100 set aside this month"), "{titles:?}");
    assert!(titles.contains(&"New income: Side Income"), "{titles:?}");
    assert!(titles.iter().any(|t| t.starts_with("You reached Hawaii")), "{titles:?}");
}

#[test]
fn usual_spending_ignores_months_before_the_book_began() {
    let mut b = both();
    spend(&mut b.s, d(3, 5), "H6210", b.home_checking, 300_00, None);
    spend(&mut b.s, d(4, 5), "H6210", b.home_checking, 100_00, None);
    let o = b.s.opportunities("2026-04").unwrap();
    assert_eq!(o.categories[0].usual, Cents(300_00), "March is the only month before April with any spending");
}

#[test]
fn the_score_paces_like_the_month_does() {
    let mut b = both();
    b.s.set_available("2026-03", Some(Cents(3200_00))).unwrap();
    spend(&mut b.s, d(3, 1), "H6000", b.home_checking, 1495_00, None);
    let s = b.s.budget_score("2026-03", d(3, 5)).unwrap();
    let pace = s.factors.iter().find(|f| f.key == "pace").unwrap();
    assert_eq!(pace.summary, "on pace for $1,495 of $3,200", "rent on the 1st isn't projected");
}
