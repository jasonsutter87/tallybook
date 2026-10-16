// Amounts are written as dollars_cents (e.g. 1200_00), hence the grouping.
#![allow(clippy::inconsistent_digit_grouping)]

use chrono::NaiveDate;
use tally_core::bank::{FeedBatch, FeedTransaction};
use tally_core::{AccountId, AccountSubtype, Cents};
use tally_store::*;

fn d(m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, day).unwrap()
}

struct Book {
    s: Store,
    checking: AccountId,
    card: AccountId,
    services: AccountId,
    sales: AccountId,
    software: AccountId,
    ar: AccountId,
    ap: AccountId,
    tax: AccountId,
    acme: ContactId,
    adobe: ContactId,
}

fn book() -> Book {
    let mut s = Store::open_in_memory().unwrap();
    s.seed_default_chart().unwrap();
    let checking = s
        .create_account(&NewAccount { code: Some("1000".into()), name: "Checking".into(), subtype: AccountSubtype::Bank, parent_id: None })
        .unwrap();
    let card = s
        .create_account(&NewAccount { code: Some("2000".into()), name: "Amex".into(), subtype: AccountSubtype::CreditCard, parent_id: None })
        .unwrap();
    let contact = |s: &Store, kind, name: &str| {
        s.create_contact(&NewContact { kind, name: name.into(), company: None, email: None, phone: None, address: None, notes: None }).unwrap()
    };
    let acme = contact(&s, ContactKind::Customer, "Acme Co");
    let adobe = contact(&s, ContactKind::Vendor, "Adobe");
    let id = |s: &Store, c: &str| s.account_by_code(c).unwrap().unwrap().id;
    Book {
        services: id(&s, "4100"),
        sales: id(&s, "4000"),
        software: id(&s, "6500"),
        ar: id(&s, "1100"),
        ap: id(&s, "2100"),
        tax: id(&s, "2200"),
        s,
        checking,
        card,
        acme,
        adobe,
    }
}

fn draft(b: &Book) -> InvoiceDraft {
    InvoiceDraft {
        customer_id: b.acme,
        issue_date: d(3, 1),
        due_date: d(3, 31),
        memo: "Thanks!".into(),
        tax_rate_bp: 825,
        lines: vec![
            InvoiceLineInput { description: "Design sprint".into(), quantity_milli: 10_000, unit_price: Cents(150_00), account_id: b.services, taxable: false },
            InvoiceLineInput { description: "Stickers".into(), quantity_milli: 2_000, unit_price: Cents(50_00), account_id: b.sales, taxable: true },
        ],
    }
}

#[test]
fn invoice_lifecycle_posts_the_right_entries() {
    let mut b = book();
    let id = b.s.create_invoice(&draft(&b)).unwrap();
    let inv = b.s.invoice(id, d(3, 1)).unwrap();
    assert_eq!(inv.summary.number, "1001");
    assert_eq!(inv.summary.status, DocStatus::Draft);
    assert_eq!(inv.subtotal, Cents(1600_00));
    assert_eq!(inv.tax, Cents(8_25));
    assert_eq!(inv.summary.total, Cents(1608_25));
    // Drafts don't touch the books.
    assert_eq!(b.s.account_balance(b.ar, d(12, 31)).unwrap(), Cents(0));

    let mut edited = draft(&b);
    edited.lines.pop();
    edited.tax_rate_bp = 0;
    b.s.update_invoice(id, &edited).unwrap();
    b.s.update_invoice(id, &draft(&b)).unwrap();

    b.s.finalize_invoice(id).unwrap();
    assert!(b.s.update_invoice(id, &draft(&b)).is_err(), "issued invoices are frozen");
    assert_eq!(b.s.account_balance(b.ar, d(3, 1)).unwrap(), Cents(1608_25));
    assert_eq!(b.s.account_balance(b.services, d(3, 1)).unwrap(), Cents(1500_00));
    assert_eq!(b.s.account_balance(b.tax, d(3, 1)).unwrap(), Cents(8_25));
    assert_eq!(b.s.invoice(id, d(4, 15)).unwrap().summary.status, DocStatus::Overdue);

    // Second invoice gets the next number.
    let id2 = b.s.create_invoice(&draft(&b)).unwrap();
    assert_eq!(b.s.invoice(id2, d(3, 1)).unwrap().summary.number, "1002");
    b.s.delete_draft_invoice(id2).unwrap();
}

#[test]
fn partial_and_full_payments() {
    let mut b = book();
    let id = b.s.create_invoice(&draft(&b)).unwrap();
    b.s.finalize_invoice(id).unwrap();

    let pay = |amount| NewPayment {
        contact_id: b.acme,
        date: d(3, 10),
        account_id: b.checking,
        method: "ACH".into(),
        reference: "".into(),
        memo: "".into(),
        applications: vec![Application { document_id: id, amount }],
    };
    let p1 = b.s.receive_payment(&pay(Cents(600_00))).unwrap();
    let inv = b.s.invoice(id, d(3, 10)).unwrap();
    assert_eq!(inv.summary.status, DocStatus::Partial);
    assert_eq!(inv.summary.balance, Cents(1008_25));
    assert!(b.s.receive_payment(&pay(Cents(2000_00))).is_err(), "overpayment rejected");

    b.s.receive_payment(&pay(Cents(1008_25))).unwrap();
    assert_eq!(b.s.invoice(id, d(3, 10)).unwrap().summary.status, DocStatus::Paid);
    assert_eq!(b.s.account_balance(b.ar, d(3, 31)).unwrap(), Cents(0));
    assert_eq!(b.s.account_balance(b.checking, d(3, 31)).unwrap(), Cents(1608_25));
    assert_eq!(b.s.contact(b.acme).unwrap().open_balance, Cents(0));

    assert!(b.s.void_invoice(id, d(3, 20)).is_err(), "paid invoices can't be voided");
    b.s.void_payment(p1, d(3, 20)).unwrap();
    let inv = b.s.invoice(id, d(3, 20)).unwrap();
    assert_eq!(inv.summary.balance, Cents(600_00));
    assert_eq!(inv.payments.len(), 1);
    assert!(b.s.balance_sheet(d(3, 31)).unwrap().is_balanced());
}

#[test]
fn customer_payment_becomes_a_bank_feed_match() {
    let mut b = book();
    let id = b.s.create_invoice(&draft(&b)).unwrap();
    b.s.finalize_invoice(id).unwrap();
    b.s.receive_payment(&NewPayment {
        contact_id: b.acme,
        date: d(3, 10),
        account_id: b.checking,
        method: "Check".into(),
        reference: "#4411".into(),
        memo: "".into(),
        applications: vec![Application { document_id: id, amount: Cents(1608_25) }],
    })
    .unwrap();
    let deposit = FeedTransaction {
        external_id: "dep".into(),
        date: d(3, 12),
        amount: Cents(1608_25),
        description: "MOBILE DEPOSIT".into(),
        merchant: None,
        pending: false,
        category_hint: None,
    };
    b.s.import_feed(b.checking, &FeedBatch { added: vec![deposit], ..Default::default() }).unwrap();
    let item = &b.s.inbox(None).unwrap()[0];
    assert!(matches!(item.suggestion, Some(Suggestion::Match { .. })), "{:?}", item.suggestion);
}

#[test]
fn bills_and_bill_payments() {
    let mut b = book();
    let bill = b
        .s
        .create_bill(&NewBill {
            vendor_id: b.adobe,
            number: "INV-889".into(),
            issue_date: d(2, 1),
            due_date: d(2, 15),
            memo: "".into(),
            lines: vec![BillLineInput { description: "Creative Cloud".into(), amount: Cents(59_99), account_id: b.software }],
        })
        .unwrap();
    assert_eq!(b.s.account_balance(b.ap, d(2, 1)).unwrap(), Cents(59_99));
    assert_eq!(b.s.contact(b.adobe).unwrap().open_balance, Cents(59_99));

    let aging = b.s.ap_aging(d(3, 1)).unwrap();
    assert_eq!(aging.totals.days_1_30, Cents(59_99));
    assert_eq!(aging.rows[0].name, "Adobe");

    // Paid with the credit card.
    b.s.pay_bills(&NewPayment {
        contact_id: b.adobe,
        date: d(2, 10),
        account_id: b.card,
        method: "Card".into(),
        reference: "".into(),
        memo: "".into(),
        applications: vec![Application { document_id: bill, amount: Cents(59_99) }],
    })
    .unwrap();
    assert_eq!(b.s.account_balance(b.ap, d(2, 28)).unwrap(), Cents(0));
    assert_eq!(b.s.account_balance(b.card, d(2, 28)).unwrap(), Cents(59_99), "now owed on the card");
    assert_eq!(b.s.bills(d(2, 28)).unwrap()[0].status, DocStatus::Paid);
    assert!(b.s.void_bill(bill, d(2, 28)).is_err());
}

#[test]
fn ar_aging_buckets_by_due_date() {
    let mut b = book();
    for (issue, due) in [(d(1, 1), d(1, 31)), (d(3, 1), d(3, 31))] {
        let mut dr = draft(&b);
        dr.issue_date = issue;
        dr.due_date = due;
        let id = b.s.create_invoice(&dr).unwrap();
        b.s.finalize_invoice(id).unwrap();
    }
    let a = b.s.ar_aging(d(3, 31)).unwrap();
    assert_eq!(a.totals.current, Cents(1608_25));
    assert_eq!(a.totals.days_31_60, Cents(1608_25));
    assert_eq!(a.rows.len(), 1);
    assert_eq!(a.rows[0].name, "Acme Co");
}

#[test]
fn guards() {
    let mut b = book();
    let mut bad = draft(&b);
    bad.customer_id = b.adobe;
    assert!(b.s.create_invoice(&bad).is_err(), "vendor isn't a customer");
    let mut bad = draft(&b);
    bad.lines[0].account_id = b.software;
    assert!(b.s.create_invoice(&bad).is_err(), "expense account on an invoice");
    let mut bad = draft(&b);
    bad.due_date = d(2, 1);
    assert!(b.s.create_invoice(&bad).is_err());
}

#[test]
fn payments_list_with_references() {
    let mut b = book();
    let id = b.s.create_invoice(&draft(&b)).unwrap();
    b.s.finalize_invoice(id).unwrap();
    let p = b
        .s
        .receive_payment(&NewPayment {
            contact_id: b.acme,
            date: d(3, 10),
            account_id: b.checking,
            method: "Check".into(),
            reference: "#4411".into(),
            memo: "".into(),
            applications: vec![Application { document_id: id, amount: Cents(100_00) }],
        })
        .unwrap();
    let list = b.s.payments(Some(PaymentKind::Received), None, d(1, 1), d(12, 31)).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].document_ids, vec![id]);
    assert_eq!(list[0].contact_name, "Acme Co");
    assert_eq!(b.s.invoice(id, d(3, 10)).unwrap().payments[0].reference, "#4411");
    b.s.void_payment(p, d(3, 11)).unwrap();
    assert!(b.s.payments(None, Some(b.acme), d(1, 1), d(12, 31)).unwrap()[0].voided);
    assert!(b.s.payments(Some(PaymentKind::Made), None, d(1, 1), d(12, 31)).unwrap().is_empty());
}
