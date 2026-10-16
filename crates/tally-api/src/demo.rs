// Amounts are written as dollars_cents (e.g. 3_200_00), hence the grouping.
#![allow(clippy::inconsistent_digit_grouping)]


//! "Try it with sample data": a small design studio with six months of
//! believable history, open invoices, bills, and a bank-feed inbox to review.

use chrono::{Datelike, Days, Months, NaiveDate};
use tally_core::bank::{FeedBatch, FeedTransaction};
use tally_core::rules::{Direction, MatchOp};
use tally_core::{AccountId, AccountSubtype, Cents};
use tally_store::*;
use std::result::Result;

/// Deterministic pseudo-random numbers so the demo looks the same every time.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo + 1) as u64) as i64
    }
}

fn code(s: &Store, c: &str) -> Result<AccountId, StoreError> {
    s.account_by_code(c)?.map(|a| a.id).ok_or(StoreError::NotFound("account code", 0))
}

pub fn seed(s: &mut Store, today: NaiveDate) -> Result<(), StoreError> {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let start = (today - Months::new(5)).with_day(1).expect("day 1");
    let opening = start - Days::new(1);

    s.set_pref("company.name", Some("Northwind Studio"))?;
    s.set_pref("company.email", Some("hello@northwind.studio"))?;
    s.set_pref("company.address", Some("418 Alder St\nPortland, OR 97205"))?;
    s.set_pref("invoice.footer", Some("Thank you for your business! Payment via ACH or check."))?;

    let checking = s.create_account(&NewAccount { code: Some("1000".into()), name: "Business Checking".into(), subtype: AccountSubtype::Bank, parent_id: None })?;
    let savings = s.create_account(&NewAccount { code: Some("1010".into()), name: "Tax Savings".into(), subtype: AccountSubtype::Bank, parent_id: None })?;
    let card = s.create_account(&NewAccount { code: Some("2000".into()), name: "Business Visa".into(), subtype: AccountSubtype::CreditCard, parent_id: None })?;
    s.post_opening_balance(checking, opening, Cents(18_250_00))?;
    s.post_opening_balance(savings, opening, Cents(9_000_00))?;
    s.post_opening_balance(card, opening, Cents(412_37))?;

    let services = code(s, "4100")?;
    let software = code(s, "6500")?;
    let rent = code(s, "6400")?;
    let meals = code(s, "6250")?;
    let contractors = code(s, "6150")?;
    let ads = code(s, "6000")?;
    let utilities = code(s, "6700")?;
    let supplies = code(s, "6300")?;
    let fees = code(s, "6100")?;
    let insurance = code(s, "6200")?;
    let draw = code(s, "3100")?;

    let contact = |s: &Store, kind, name: &str, email: &str| {
        s.create_contact(&NewContact { kind, name: name.into(), company: None, email: Some(email.into()), phone: None, address: None, notes: None })
    };
    let customers = [
        contact(s, ContactKind::Customer, "Bluebird Coffee", "ap@bluebird.coffee")?,
        contact(s, ContactKind::Customer, "Juniper Health", "billing@juniperhealth.com")?,
        contact(s, ContactKind::Customer, "Atlas Outdoor Co", "finance@atlasoutdoor.co")?,
        contact(s, ContactKind::Customer, "Fern & Finch", "hello@fernandfinch.com")?,
    ];
    contact(s, ContactKind::Vendor, "Pearl District Properties", "leasing@pearlprops.com")?;
    let freelancer = contact(s, ContactKind::Vendor, "Maya Chen Illustration", "maya@mayachen.art")?;
    s.set_contact_tax(freelancer, &ContactTax { is_1099: true, w9_on_file: true, legal_name: Some("Maya L. Chen".into()), tin_last4: Some("4821".into()) })?;
    contact(s, ContactKind::Vendor, "Figma", "billing@figma.com")?;

    // Rules a real user would have built up.
    for (pattern, target, auto) in [("figma", software, true), ("adobe", software, true), ("pge", utilities, false), ("google ads", ads, false)] {
        s.create_rule(&NewRule {
            name: pattern.to_uppercase(),
            priority: 100,
            op: MatchOp::Contains,
            pattern: pattern.into(),
            direction: Direction::Outflow,
            amount_min: None,
            amount_max: None,
            bank_account_id: None,
            target_account_id: target,
            rename_to: None,
            auto_post: auto,
        })?;
    }

    let mut ext = 0;
    let mut txn = |date: NaiveDate, cents: i64, desc: &str| {
        ext += 1;
        FeedTransaction { external_id: format!("demo-{ext}"), date, amount: Cents(cents), description: desc.into(), merchant: None, pending: false, category_hint: None }
    };

    // Six months of history, reviewed except for the last ~10 days.
    let review_cutoff = today - Days::new(16);
    let mut checking_feed = Vec::new();
    let mut card_feed = Vec::new();
    for m in 0..6 {
        let month = start + Months::new(m);
        let day = |d: u32| month.with_day(d.min(28)).expect("valid day");
        if day(1) > today {
            break;
        }
        checking_feed.push((txn(day(1), -3_200_00, "PEARL DISTRICT PROPS RENT"), Some(rent)));
        checking_feed.push((txn(day(3), -rng.range(140_00, 210_00), "PGE WEB ONLINE"), Some(utilities)));
        checking_feed.push((txn(day(5), -189_00, "HISCOX INSURANCE"), Some(insurance)));
        checking_feed.push((txn(day(15), -2_500_00, "TRANSFER TO OWNER"), Some(draw)));
        checking_feed.push((txn(day(16), -1_000_00, "TRANSFER TO TAX SAVINGS"), None));
        checking_feed.push((txn(day(28), -rng.range(5_00, 25_00), "MONTHLY SERVICE FEE"), Some(fees)));
        card_feed.push((txn(day(2), -45_00, "FIGMA MONTHLY"), Some(software)));
        card_feed.push((txn(day(4), -59_99, "ADOBE CREATIVE CLOUD"), Some(software)));
        card_feed.push((txn(day(9), -20_00, "NOTION LABS"), Some(software)));
        for _ in 0..rng.range(3, 6) {
            let d = day(rng.range(1, 28) as u32);
            let (desc, cat) = match rng.range(0, 4) {
                0 => ("STUMPTOWN COFFEE", meals),
                1 => ("NOSTRANA RESTAURANT", meals),
                2 => ("PAPER SOURCE", supplies),
                3 => ("GOOGLE ADS", ads),
                _ => ("AMAZON MKTPL", supplies),
            };
            card_feed.push((txn(d, -rng.range(12_00, 180_00), desc), Some(cat)));
        }
        card_feed.push((txn(day(20), 0, "PAYMENT THANK YOU"), None)); // amount filled below
    }

    // A few recent, unfamiliar merchants that need a human decision.
    for (back, cents, desc) in [(1, -38_40, "UBER   *TRIP HELP.UBER.COM"), (3, -412_00, "DELTA AIR 0062391"), (4, -86_25, "SQ *PRINTSHOP PDX"), (6, -24_99, "LINKEDIN PREMIUM"), (9, -149_00, "WEBFLOW.COM")] {
        let d = today - Days::new(back);
        if d >= start {
            card_feed.push((txn(d, cents, desc), None));
        }
    }

    // Invoices: each month every client gets billed; most are paid.
    let mut paid_deposits = Vec::new();
    for m in 0..6 {
        let issue = (start + Months::new(m)).with_day(6).expect("valid day");
        if issue > today {
            break;
        }
        for (i, customer) in customers.iter().enumerate() {
            if (m as usize + i) % 3 == 2 {
                continue;
            }
            let hours = rng.range(12, 48) * 1000;
            let draft = InvoiceDraft {
                customer_id: *customer,
                issue_date: issue,
                due_date: issue + Days::new(30),
                memo: "Thanks for working with Northwind!".into(),
                tax_rate_bp: 0,
                lines: vec![
                    InvoiceLineInput { description: "Brand & web design".into(), quantity_milli: hours, unit_price: Cents(135_00), account_id: services, taxable: false },
                    InvoiceLineInput { description: "Project management".into(), quantity_milli: 4000, unit_price: Cents(95_00), account_id: services, taxable: false },
                ],
            };
            let id = s.create_invoice(&draft)?;
            s.finalize_invoice(id)?;
            let inv = s.invoice(id, today)?;
            let pay_date = issue + Days::new(rng.range(12, 34) as u64);
            // Recent ones (and one older straggler) stay open.
            if pay_date < today - Days::new(5) && !(m == 3 && i == 1) {
                s.receive_payment(&NewPayment {
                    contact_id: *customer,
                    date: pay_date,
                    account_id: checking,
                    method: "ACH".into(),
                    reference: String::new(),
                    memo: String::new(),
                    applications: vec![Application { document_id: id, amount: inv.summary.total }],
                })?;
                paid_deposits.push((pay_date, inv.summary.total));
            }
        }
    }
    // A draft for the user to finish.
    s.create_invoice(&InvoiceDraft {
        customer_id: customers[3],
        issue_date: today,
        due_date: today + Days::new(30),
        memo: String::new(),
        tax_rate_bp: 0,
        lines: vec![InvoiceLineInput { description: "Packaging refresh — phase 1".into(), quantity_milli: 20_000, unit_price: Cents(135_00), account_id: services, taxable: false }],
    })?;

    // Bills: freelancer monthly, one upcoming.
    for m in 0..6 {
        let issue = (start + Months::new(m)).with_day(12).expect("valid day");
        if issue > today {
            break;
        }
        let amount = Cents(rng.range(900_00, 2_200_00));
        let bill = s.create_bill(&NewBill {
            vendor_id: freelancer,
            number: format!("MC-{}", 300 + m),
            issue_date: issue,
            due_date: issue + Days::new(15),
            memo: "Illustration".into(),
            lines: vec![BillLineInput { description: "Illustration work".into(), amount, account_id: contractors }],
        })?;
        let pay_date = issue + Days::new(14);
        if pay_date < today - Days::new(3) {
            s.pay_bills(&NewPayment {
                contact_id: freelancer,
                date: pay_date,
                account_id: checking,
                method: "ACH".into(),
                reference: String::new(),
                memo: String::new(),
                applications: vec![Application { document_id: bill, amount }],
            })?;
            checking_feed.push((txn(pay_date + Days::new(1), -amount.0, "MAYA CHEN ACH"), None));
        }
    }
    for (date, amount) in &paid_deposits {
        checking_feed.push((txn(*date + Days::new(1), amount.0, "ACH DEPOSIT"), None));
    }

    // Card payments settle each month's card spend from checking.
    let mut month_spend: std::collections::BTreeMap<(i32, u32), i64> = Default::default();
    for (t, _) in &card_feed {
        if t.amount.0 < 0 {
            *month_spend.entry((t.date.year(), t.date.month())).or_default() += -t.amount.0;
        }
    }
    for (t, _) in card_feed.iter_mut() {
        if t.description == "PAYMENT THANK YOU" {
            let prev = t.date - Months::new(1);
            let owed = month_spend.get(&(prev.year(), prev.month())).copied().unwrap_or(412_37);
            t.amount = Cents(owed);
            checking_feed.push((txn(t.date, -owed, "VISA AUTOPAY"), None));
        }
    }

    // Import, then review history the way a diligent owner would have.
    for (account, feed) in [(checking, checking_feed), (card, card_feed)] {
        let batch = FeedBatch { added: feed.iter().map(|(t, _)| t.clone()).filter(|t| t.date <= today).collect(), ..Default::default() };
        s.import_feed(account, &batch)?;
        let inbox = s.inbox(Some(account))?;
        for item in inbox {
            if item.txn.date > review_cutoff {
                continue;
            }
            let category = feed.iter().find(|(t, _)| t.external_id == item.txn.external_id).and_then(|(_, c)| *c);
            match (&item.suggestion, category) {
                (Some(Suggestion::Match { line_id, .. }), _) => s.match_txn(item.txn.id, *line_id)?,
                (_, Some(cat)) => {
                    s.accept(item.txn.id, cat, None)?;
                }
                (_, None) if item.txn.description == "TRANSFER TO TAX SAVINGS" => {
                    s.accept(item.txn.id, savings, Some("Transfer to Tax Savings".into()))?;
                }
                // Post the checking side as a transfer; the card side then
                // arrives as a match for that same entry, not a second posting.
                (_, None) if item.txn.description == "VISA AUTOPAY" => {
                    s.accept(item.txn.id, card, Some("Credit card payment".into()))?;
                }
                _ => {}
            }
        }
    }
    // A tax profile and the estimated payments a careful owner would have made.
    let profile = serde_json::json!({
        "filing_status": "single",
        "method": "safe_harbor",
        "other_income": 0,
        "w2_wages": 0,
        "withholding": 0,
        "last_year_tax": 24_600_00,
        "last_year_agi": 118_000_00,
        "projected_profit": null,
        "home_office_sqft": 0,
        "state_rate_bp": 900
    });
    s.set_pref("tax.profile", Some(&profile.to_string()))?;
    let year = today.year();
    for (q, due) in tally_core::tax::quarterly_due_dates(year).iter().enumerate() {
        if *due < today && *due >= start {
            s.record_tax_payment(&NewTaxPayment {
                tax_year: year,
                quarter: q as u8 + 1,
                jurisdiction: Jurisdiction::Federal,
                date: *due,
                amount: Cents(6_150_00),
                account_id: checking,
            })?;
        }
    }
    Ok(())
}
