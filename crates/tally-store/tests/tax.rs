// Amounts are written as dollars_cents (e.g. 1200_00), hence the grouping.
#![allow(clippy::inconsistent_digit_grouping)]

use chrono::NaiveDate;
use tally_core::tax::ScheduleCLine;
use tally_core::{AccountId, AccountSubtype, Cents, EntryLine, NewEntry};
use tally_store::*;

fn d(m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, day).unwrap()
}

struct Book {
    s: Store,
    checking: AccountId,
    card: AccountId,
    services: AccountId,
    meals: AccountId,
    contractors: AccountId,
}

fn book() -> Book {
    let mut s = Store::open_in_memory().unwrap();
    s.seed_default_chart().unwrap();
    let checking = s.create_account(&NewAccount::new("Checking", AccountSubtype::Bank).code("1000")).unwrap();
    let card = s.create_account(&NewAccount::new("Visa", AccountSubtype::CreditCard).code("2000")).unwrap();
    let id = |s: &Store, c: &str| s.account_by_code(c).unwrap().unwrap().id;
    Book { services: id(&s, "4100"), meals: id(&s, "6250"), contractors: id(&s, "6150"), s, checking, card }
}

fn post(s: &mut Store, date: NaiveDate, debit: AccountId, credit: AccountId, cents: i64) -> i64 {
    s.post_entry(&NewEntry::new(date, "x").line(EntryLine::debit(debit, Cents(cents))).line(EntryLine::credit(credit, Cents(cents)))).unwrap()
}

fn contact(s: &Store, name: &str) -> i64 {
    s.create_contact(&NewContact { kind: ContactKind::Vendor, name: name.into(), company: None, email: None, phone: None, address: None, notes: None }).unwrap()
}

#[test]
fn schedule_c_uses_suggestions_until_confirmed() {
    let mut b = book();
    post(&mut b.s, d(2, 1), b.checking, b.services, 10_000_00);
    post(&mut b.s, d(2, 2), b.meals, b.checking, 200_00);
    let r = b.s.schedule_c(2026, Cents::ZERO).unwrap();
    assert_eq!(r.schedule.line1_gross_receipts, Cents(10_000_00));
    assert_eq!(r.schedule.meals_disallowed, Cents(100_00));
    assert_eq!(r.schedule.line31_net_profit, Cents(9_900_00));
    assert_eq!(r.unconfirmed, 2);

    // The user decides those meals were really client gifts → other expenses.
    b.s.set_tax_line(b.meals, Some(ScheduleCLine::L27aOther)).unwrap();
    b.s.set_tax_line(b.services, Some(ScheduleCLine::L1GrossReceipts)).unwrap();
    let r = b.s.schedule_c(2026, Cents(500_00)).unwrap();
    assert_eq!(r.unconfirmed, 0);
    assert_eq!(r.schedule.meals_disallowed, Cents::ZERO);
    assert_eq!(r.schedule.line31_net_profit, Cents(10_000_00 - 200_00 - 500_00));
    assert!(b.s.set_tax_line(b.checking, Some(ScheduleCLine::L22Supplies)).is_err(), "balance-sheet accounts aren't on Schedule C");
    // Other years are separate.
    assert_eq!(b.s.schedule_c(2025, Cents::ZERO).unwrap().schedule.line1_gross_receipts, Cents::ZERO);
}

#[test]
fn report_1099_counts_bank_payments_not_card() {
    let mut b = book();
    let maya = contact(&b.s, "Maya Chen");
    let small = contact(&b.s, "Tiny Gig");
    b.s.set_contact_tax(maya, &ContactTax { is_1099: true, w9_on_file: true, legal_name: Some("Maya L Chen".into()), tin_last4: Some("1234".into()) }).unwrap();
    b.s.set_contact_tax(small, &ContactTax { is_1099: true, w9_on_file: false, legal_name: None, tin_last4: None }).unwrap();

    let e1 = post(&mut b.s, d(3, 1), b.contractors, b.checking, 1_500_00); // ACH
    let e2 = post(&mut b.s, d(4, 1), b.contractors, b.card, 900_00); // card: not reportable by us
    let e3 = post(&mut b.s, d(5, 1), b.contractors, b.checking, 700_00);
    let e4 = post(&mut b.s, d(5, 2), b.contractors, b.checking, 150_00);
    for e in [e1, e2, e3] {
        b.s.set_entry_payee(e, Some(maya)).unwrap();
    }
    b.s.set_entry_payee(e4, Some(small)).unwrap();
    // A voided payment doesn't count.
    let e5 = post(&mut b.s, d(6, 1), b.contractors, b.checking, 5_000_00);
    b.s.set_entry_payee(e5, Some(maya)).unwrap();
    b.s.void_entry(e5, d(6, 2)).unwrap();

    let r = b.s.report_1099(2026).unwrap();
    assert_eq!(r.threshold, Cents(2_000_00));
    let m = r.rows.iter().find(|p| p.contact_id == maya).unwrap();
    assert_eq!(m.paid_reportable, Cents(2_200_00));
    assert_eq!(m.paid_by_card, Cents(900_00));
    assert!(m.over_threshold);
    let t = r.rows.iter().find(|p| p.contact_id == small).unwrap();
    assert!(!t.over_threshold);
    assert!(b.s.set_contact_tax(maya, &ContactTax { is_1099: true, w9_on_file: true, legal_name: None, tin_last4: Some("12-4".into()) }).is_err());
    assert!(b.s.report_1099(2019).is_err(), "no tables, no guess");
}

#[test]
fn bill_payments_carry_their_vendor_as_payee() {
    let mut b = book();
    let maya = contact(&b.s, "Maya Chen");
    b.s.set_contact_tax(maya, &ContactTax { is_1099: true, w9_on_file: true, legal_name: None, tin_last4: None }).unwrap();
    let bill = b
        .s
        .create_bill(&NewBill {
            vendor_id: maya,
            number: "7".into(),
            issue_date: d(2, 1),
            due_date: d(2, 15),
            memo: String::new(),
            lines: vec![BillLineInput { description: "Illustration".into(), amount: Cents(2_500_00), account_id: b.contractors }],
        })
        .unwrap();
    b.s.pay_bills(&NewPayment {
        contact_id: maya,
        date: d(2, 10),
        account_id: b.checking,
        method: "ACH".into(),
        reference: String::new(),
        memo: String::new(),
        applications: vec![Application { document_id: bill, amount: Cents(2_500_00) }],
    })
    .unwrap();
    let r = b.s.report_1099(2026).unwrap();
    assert_eq!(r.rows[0].paid_reportable, Cents(2_500_00), "the bill itself moves no bank money; the payment does");
}

#[test]
fn estimated_payments_go_to_owners_draw_not_expenses() {
    let mut b = book();
    let id = b
        .s
        .record_tax_payment(&NewTaxPayment { tax_year: 2026, quarter: 2, jurisdiction: Jurisdiction::Federal, date: d(6, 15), amount: Cents(3_000_00), account_id: b.checking })
        .unwrap();
    let draw = b.s.account_by_code("3100").unwrap().unwrap().id;
    assert_eq!(b.s.account_balance(draw, d(12, 31)).unwrap(), Cents(-3_000_00), "a draw reduces equity");
    assert_eq!(b.s.profit_and_loss(d(1, 1), d(12, 31)).unwrap().total_expenses, Cents::ZERO);
    assert_eq!(b.s.tax_payments(2026).unwrap().len(), 1);
    b.s.void_tax_payment(id, d(6, 16)).unwrap();
    assert!(b.s.tax_payments(2026).unwrap()[0].voided);
    assert_eq!(b.s.account_balance(b.checking, d(12, 31)).unwrap(), Cents::ZERO);
    assert!(b.s.record_tax_payment(&NewTaxPayment { tax_year: 2026, quarter: 5, jurisdiction: Jurisdiction::State, date: d(6, 15), amount: Cents(1), account_id: b.checking }).is_err());
}
