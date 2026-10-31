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

/// A feed transaction and the category the owner filed it under (`None`: left for Review).
type FeedRow = (FeedTransaction, Option<AccountId>);

/// The accounts and people the later steps need to refer to.
struct Demo {
    today: NaiveDate,
    /// First day of the oldest month of history.
    start: NaiveDate,
    checking: AccountId,
    savings: AccountId,
    card: AccountId,
    cats: Categories,
    customers: [ContactId; 4],
    freelancer: ContactId,
}

/// The expense and income accounts of the default chart the sample uses.
struct Categories {
    services: AccountId,
    software: AccountId,
    rent: AccountId,
    meals: AccountId,
    contractors: AccountId,
    ads: AccountId,
    utilities: AccountId,
    supplies: AccountId,
    fees: AccountId,
    insurance: AccountId,
    draw: AccountId,
}

/// What the two bank accounts' feeds will deliver, built up by the steps below.
#[derive(Default)]
struct Feeds {
    checking: Vec<FeedRow>,
    card: Vec<FeedRow>,
    /// Counts up so every sample transaction gets its own external id.
    last_id: u32,
}

impl Feeds {
    fn txn(&mut self, date: NaiveDate, cents: i64, desc: &str) -> FeedTransaction {
        self.last_id += 1;
        FeedTransaction { external_id: format!("demo-{}", self.last_id), date, amount: Cents(cents), description: desc.into(), merchant: None, pending: false, category_hint: None }
    }
}

pub fn seed(s: &mut Store, today: NaiveDate) -> Result<(), StoreError> {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let start = (today - Months::new(5)).with_day(1).expect("day 1");

    seed_company(s)?;
    let (checking, savings, card) = seed_bank_accounts(s, start - Days::new(1))?;
    let cats = lookup_categories(s)?;
    let (customers, freelancer) = seed_contacts(s)?;
    let demo = Demo { today, start, checking, savings, card, cats, customers, freelancer };
    seed_rules(s, &demo.cats)?;

    let mut feeds = Feeds::default();
    history_feeds(&mut rng, &demo, &mut feeds);
    let paid_deposits = seed_invoices(s, &mut rng, &demo)?;
    seed_freelancer_bills(s, &mut rng, &demo, &mut feeds)?;
    for (date, amount) in &paid_deposits {
        let t = feeds.txn(*date + Days::new(1), amount.0, "ACH DEPOSIT");
        feeds.checking.push((t, None));
    }
    settle_card_payments(&mut feeds);
    import_and_review(s, &demo, feeds)?;
    seed_tax(s, &demo)
}

fn seed_company(s: &Store) -> Result<(), StoreError> {
    s.set_pref("company.name", Some("Northwind Studio"))?;
    s.set_pref("company.email", Some("hello@northwind.studio"))?;
    s.set_pref("company.address", Some("418 Alder St\nPortland, OR 97205"))?;
    s.set_pref("invoice.footer", Some("Thank you for your business! Payment via ACH or check."))?;
    Ok(())
}

/// Checking, tax savings and a card, each opened with a balance the day before the history starts.
fn seed_bank_accounts(s: &mut Store, opening: NaiveDate) -> Result<(AccountId, AccountId, AccountId), StoreError> {
    let checking = s.create_account(&NewAccount::new("Business Checking", AccountSubtype::Bank).code("1000"))?;
    let savings = s.create_account(&NewAccount::new("Tax Savings", AccountSubtype::Bank).code("1010"))?;
    let card = s.create_account(&NewAccount::new("Business Visa", AccountSubtype::CreditCard).code("2000"))?;
    s.post_opening_balance(checking, opening, Cents(18_250_00))?;
    s.post_opening_balance(savings, opening, Cents(9_000_00))?;
    s.post_opening_balance(card, opening, Cents(412_37))?;
    Ok((checking, savings, card))
}

fn lookup_categories(s: &Store) -> Result<Categories, StoreError> {
    Ok(Categories {
        services: code(s, "4100")?,
        software: code(s, "6500")?,
        rent: code(s, "6400")?,
        meals: code(s, "6250")?,
        contractors: code(s, "6150")?,
        ads: code(s, "6000")?,
        utilities: code(s, "6700")?,
        supplies: code(s, "6300")?,
        fees: code(s, "6100")?,
        insurance: code(s, "6200")?,
        draw: code(s, "3100")?,
    })
}

fn contact(s: &Store, kind: ContactKind, name: &str, email: &str) -> Result<ContactId, StoreError> {
    s.create_contact(&NewContact { kind, name: name.into(), company: None, email: Some(email.into()), phone: None, address: None, notes: None })
}

/// Four customers, and the vendors (one of them a 1099 freelancer).
fn seed_contacts(s: &Store) -> Result<([ContactId; 4], ContactId), StoreError> {
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
    Ok((customers, freelancer))
}

/// Rules a real user would have built up.
fn seed_rules(s: &mut Store, cats: &Categories) -> Result<(), StoreError> {
    for (pattern, target, auto) in [("figma", cats.software, true), ("adobe", cats.software, true), ("pge", cats.utilities, false), ("google ads", cats.ads, false)] {
        s.create_rule(&NewRule {
            name: pattern.to_uppercase(),
            priority: tally_core::rules::DEFAULT_PRIORITY,
            op: MatchOp::Contains,
            pattern: pattern.into(),
            direction: Direction::Outflow,
            amount_min: None,
            amount_max: None,
            bank_account_id: None,
            target_account_id: target,
            rename_to: None,
            auto_post: auto,
            person_id: None,
        })?;
    }
    Ok(())
}

/// Six months of bank-feed history (the card payments' amounts are filled in
/// later), plus a few recent unfamiliar merchants that need a human decision.
fn history_feeds(rng: &mut Rng, d: &Demo, feeds: &mut Feeds) {
    let cats = &d.cats;
    for m in 0..6 {
        let month = d.start + Months::new(m);
        let day = |n: u32| month.with_day(n.min(28)).expect("valid day");
        if day(1) > d.today {
            break;
        }
        let t = feeds.txn(day(1), -3_200_00, "PEARL DISTRICT PROPS RENT");
        feeds.checking.push((t, Some(cats.rent)));
        let t = feeds.txn(day(3), -rng.range(140_00, 210_00), "PGE WEB ONLINE");
        feeds.checking.push((t, Some(cats.utilities)));
        let t = feeds.txn(day(5), -189_00, "HISCOX INSURANCE");
        feeds.checking.push((t, Some(cats.insurance)));
        let t = feeds.txn(day(15), -2_500_00, "TRANSFER TO OWNER");
        feeds.checking.push((t, Some(cats.draw)));
        let t = feeds.txn(day(16), -1_000_00, "TRANSFER TO TAX SAVINGS");
        feeds.checking.push((t, None));
        let t = feeds.txn(day(28), -rng.range(5_00, 25_00), "MONTHLY SERVICE FEE");
        feeds.checking.push((t, Some(cats.fees)));
        let t = feeds.txn(day(2), -45_00, "FIGMA MONTHLY");
        feeds.card.push((t, Some(cats.software)));
        let t = feeds.txn(day(4), -59_99, "ADOBE CREATIVE CLOUD");
        feeds.card.push((t, Some(cats.software)));
        let t = feeds.txn(day(9), -20_00, "NOTION LABS");
        feeds.card.push((t, Some(cats.software)));
        for _ in 0..rng.range(3, 6) {
            let date = day(rng.range(1, 28) as u32);
            let (desc, cat) = match rng.range(0, 4) {
                0 => ("STUMPTOWN COFFEE", cats.meals),
                1 => ("NOSTRANA RESTAURANT", cats.meals),
                2 => ("PAPER SOURCE", cats.supplies),
                3 => ("GOOGLE ADS", cats.ads),
                _ => ("AMAZON MKTPL", cats.supplies),
            };
            let t = feeds.txn(date, -rng.range(12_00, 180_00), desc);
            feeds.card.push((t, Some(cat)));
        }
        let t = feeds.txn(day(20), 0, "PAYMENT THANK YOU"); // amount filled in by settle_card_payments
        feeds.card.push((t, None));
    }

    for (back, cents, desc) in [(1, -38_40, "UBER   *TRIP HELP.UBER.COM"), (3, -412_00, "DELTA AIR 0062391"), (4, -86_25, "SQ *PRINTSHOP PDX"), (6, -24_99, "LINKEDIN PREMIUM"), (9, -149_00, "WEBFLOW.COM")] {
        let date = d.today - Days::new(back);
        if date >= d.start {
            let t = feeds.txn(date, cents, desc);
            feeds.card.push((t, None));
        }
    }
}

/// Each month every client gets billed and most pay; returns the payments
/// received, so the bank feed can show them as deposits.
fn seed_invoices(s: &mut Store, rng: &mut Rng, d: &Demo) -> Result<Vec<(NaiveDate, Cents)>, StoreError> {
    let services = d.cats.services;
    let mut paid_deposits = Vec::new();
    for m in 0..6 {
        let issue = (d.start + Months::new(m)).with_day(6).expect("valid day");
        if issue > d.today {
            break;
        }
        for (i, customer) in d.customers.iter().enumerate() {
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
            let inv = s.invoice(id, d.today)?;
            let pay_date = issue + Days::new(rng.range(12, 34) as u64);
            // Recent ones (and one older straggler) stay open.
            if pay_date < d.today - Days::new(5) && !(m == 3 && i == 1) {
                s.receive_payment(&NewPayment {
                    contact_id: *customer,
                    date: pay_date,
                    account_id: d.checking,
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
        customer_id: d.customers[3],
        issue_date: d.today,
        due_date: d.today + Days::new(30),
        memo: String::new(),
        tax_rate_bp: 0,
        lines: vec![InvoiceLineInput { description: "Packaging refresh — phase 1".into(), quantity_milli: 20_000, unit_price: Cents(135_00), account_id: services, taxable: false }],
    })?;
    Ok(paid_deposits)
}

/// The freelancer bills monthly; the paid ones also show up in the checking feed.
fn seed_freelancer_bills(s: &mut Store, rng: &mut Rng, d: &Demo, feeds: &mut Feeds) -> Result<(), StoreError> {
    let issue_date = |m: u32| (d.start + Months::new(m)).with_day(12).expect("valid day");
    // The newest bill always stays open, so A/P has something to age on every
    // day of the month (paying it on the 26th left nothing open from the 30th
    // through the 11th).
    let newest = (0..6).rev().find(|&m| issue_date(m) <= d.today);
    for m in 0..6 {
        let issue = issue_date(m);
        if issue > d.today {
            break;
        }
        let amount = Cents(rng.range(900_00, 2_200_00));
        let bill = s.create_bill(&NewBill {
            vendor_id: d.freelancer,
            number: format!("MC-{}", 300 + m),
            issue_date: issue,
            due_date: issue + Days::new(15),
            memo: "Illustration".into(),
            lines: vec![BillLineInput { description: "Illustration work".into(), amount, account_id: d.cats.contractors }],
        })?;
        let pay_date = issue + Days::new(14);
        if Some(m) != newest && pay_date < d.today - Days::new(3) {
            s.pay_bills(&NewPayment {
                contact_id: d.freelancer,
                date: pay_date,
                account_id: d.checking,
                method: "ACH".into(),
                reference: String::new(),
                memo: String::new(),
                applications: vec![Application { document_id: bill, amount }],
            })?;
            let t = feeds.txn(pay_date + Days::new(1), -amount.0, "MAYA CHEN ACH");
            feeds.checking.push((t, None));
        }
    }
    Ok(())
}

/// Card payments settle each month's card spend from checking.
fn settle_card_payments(feeds: &mut Feeds) {
    let mut month_spend: std::collections::BTreeMap<(i32, u32), i64> = Default::default();
    for (t, _) in &feeds.card {
        if t.amount.0 < 0 {
            *month_spend.entry((t.date.year(), t.date.month())).or_default() += -t.amount.0;
        }
    }
    let mut autopays = Vec::new();
    for (t, _) in feeds.card.iter_mut() {
        if t.description == "PAYMENT THANK YOU" {
            let prev = t.date - Months::new(1);
            let owed = month_spend.get(&(prev.year(), prev.month())).copied().unwrap_or(412_37);
            t.amount = Cents(owed);
            autopays.push((t.date, owed));
        }
    }
    for (date, owed) in autopays {
        let t = feeds.txn(date, -owed, "VISA AUTOPAY");
        feeds.checking.push((t, None));
    }
}

/// Imports the feeds, then reviews all but the last ~10 days the way a diligent owner would have.
fn import_and_review(s: &mut Store, d: &Demo, feeds: Feeds) -> Result<(), StoreError> {
    let review_cutoff = d.today - Days::new(16);
    for (account, feed) in [(d.checking, feeds.checking), (d.card, feeds.card)] {
        let batch = FeedBatch { added: feed.iter().map(|(t, _)| t.clone()).filter(|t| t.date <= d.today).collect(), ..Default::default() };
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
                    s.accept(item.txn.id, d.savings, Some("Transfer to Tax Savings".into()))?;
                }
                // Post the checking side as a transfer; the card side then
                // arrives as a match for that same entry, not a second posting.
                (_, None) if item.txn.description == "VISA AUTOPAY" => {
                    s.accept(item.txn.id, d.card, Some("Credit card payment".into()))?;
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// A tax profile and the estimated payments a careful owner would have made.
fn seed_tax(s: &mut Store, d: &Demo) -> Result<(), StoreError> {
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
    let year = d.today.year();
    for (q, due) in tally_core::tax::quarterly_due_dates(year).iter().enumerate() {
        if *due < d.today {
            s.record_tax_payment(&NewTaxPayment {
                tax_year: year,
                quarter: q as u8 + 1,
                jurisdiction: Jurisdiction::Federal,
                // The sample's books start mid-year; earlier payments land on day one.
                date: (*due).max(d.start),
                amount: Cents(6_150_00),
                account_id: d.checking,
            })?;
        }
    }
    Ok(())
}
