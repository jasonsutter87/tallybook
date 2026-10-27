//! Helpers shared by the sync tests: devices joined to one in-memory server,
//! a seeded random-number generator, and random bookkeeping operations.
#![allow(dead_code, clippy::inconsistent_digit_grouping)]

use chrono::NaiveDate;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use tally_core::bank::{FeedBatch, FeedTransaction};
use tally_core::rules::{Direction, MatchOp};
use tally_core::{Account, AccountId, AccountSubtype, Cents, EntryLine, NewEntry};
use tally_store::*;
use tally_sync::*;

pub fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

pub fn temp_dir() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("tally-sync-test-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn code(s: &Store, c: &str) -> AccountId {
    s.account_by_code(c).unwrap().unwrap().id
}

pub struct Device {
    pub store: Store,
    pub net: MemoryTransport,
}

impl Device {
    pub fn push(&mut self, key: &SyncKey) -> std::result::Result<(usize, usize), SyncError> {
        push_outbox(&self.store, &mut self.net, key)
    }

    pub fn snapshot(&mut self, key: &SyncKey) -> std::result::Result<u64, SyncError> {
        create_snapshot(&self.store, &mut self.net, key)
    }

    pub fn sync(&mut self, key: &SyncKey) -> std::result::Result<SyncReport, SyncError> {
        sync_once(&mut self.store, &mut self.net, key)
    }
}

/// One book on several devices, joined through one server.
pub struct World {
    pub server: MemoryServer,
    pub key: SyncKey,
    pub devs: Vec<Device>,
    pub dir: PathBuf,
}

impl World {
    /// `n` devices that already agree: the original (with a chart and two
    /// accounts) uploaded a snapshot, and the rest joined from it.
    pub fn new(n: usize) -> World {
        let dir = temp_dir();
        let server = MemoryServer::new();
        let key = SyncKey::generate().unwrap();
        let mut store = Store::open_in_memory().unwrap();
        store.seed_default_chart().unwrap();
        store.create_account(&NewAccount::new("Checking", AccountSubtype::Bank).code("1000")).unwrap();
        store.create_account(&NewAccount::new("Visa", AccountSubtype::CreditCard).code("2000")).unwrap();
        store.sync_enable().unwrap();
        let mut net = server.connect();
        create_snapshot(&store, &mut net, &key).unwrap();
        let mut devs = vec![Device { store, net }];
        for i in 1..n {
            let mut net = server.connect();
            let store = join_from_snapshot(&mut net, &key, &dir.join(format!("device-{i}.tallybook"))).unwrap();
            // Tests have no need to survive a power cut.
            store.debug_execute("PRAGMA synchronous = OFF").unwrap();
            devs.push(Device { store, net });
        }
        World { server, key, devs, dir }
    }

    /// Every device syncs, in the given order.
    pub fn sync_in_order(&mut self, order: &[usize]) -> Vec<SyncReport> {
        order.iter().map(|&i| self.devs[i].sync(&self.key).unwrap()).collect()
    }

    /// Syncs every device twice over, so everything has reached everyone.
    pub fn sync_everyone(&mut self) -> Vec<SyncReport> {
        let order: Vec<usize> = (0..self.devs.len()).chain(0..self.devs.len()).collect();
        self.sync_in_order(&order)
    }

    pub fn assert_converged(&self) {
        assert_same_books(self.devs.iter().map(|d| &d.store).collect::<Vec<_>>().as_slice());
    }

    pub fn finish(self) {
        drop(self.devs);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Every synced table identical, row for row, and the same balanced trial balance.
pub fn assert_same_books(stores: &[&Store]) {
    let first = stores[0].sync_dump().unwrap();
    for (i, other) in stores.iter().enumerate().skip(1) {
        let dump = other.sync_dump().unwrap();
        for (table, rows) in &first {
            if rows != &dump[table] {
                let (a, b) = (&dump[table], rows);
                let only_there: Vec<&String> = a.iter().filter(|r| !b.contains(r)).collect();
                let only_here: Vec<&String> = b.iter().filter(|r| !a.contains(r)).collect();
                panic!("table {table} differs between device 0 and device {i}\n  only on device 0: {only_here:#?}\n  only on device {i}: {only_there:#?}");
            }
        }
    }
    let far = d(2099, 12, 31);
    let tb = stores[0].trial_balance(far).unwrap();
    assert!(tb.is_balanced(), "the books must balance");
    for other in &stores[1..] {
        assert_eq!(other.trial_balance(far).unwrap(), tb);
    }
}

pub fn post(s: &mut Store, date: NaiveDate, memo: &str, debit: AccountId, credit: AccountId, cents: i64) -> i64 {
    s.post_entry(&NewEntry::new(date, memo).line(EntryLine::debit(debit, Cents(cents))).line(EntryLine::credit(credit, Cents(cents)))).unwrap()
}

pub fn customer(s: &Store, name: &str) -> i64 {
    s.create_contact(&NewContact { kind: ContactKind::Customer, name: name.into(), company: None, email: None, phone: None, address: None, notes: None })
        .unwrap()
}

pub fn invoice(s: &mut Store, customer: i64) -> i64 {
    let sales = code(s, "4000");
    s.create_invoice(&InvoiceDraft {
        customer_id: customer,
        issue_date: d(2026, 3, 1),
        due_date: d(2026, 3, 31),
        memo: String::new(),
        tax_rate_bp: 0,
        lines: vec![InvoiceLineInput { description: "Work".into(), quantity_milli: 1000, unit_price: Cents(100_00), account_id: sales, taxable: false }],
    })
    .unwrap()
}

pub fn tick() {
    std::thread::sleep(std::time::Duration::from_millis(3));
}

// ---------------------------------------------------------------- random operations

/// xorshift64*: small, seedable, and good enough to shuffle test operations.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        (!items.is_empty()).then(|| &items[self.below(items.len())])
    }

    pub fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
}

/// One random bookkeeping operation on a store. Operations the store refuses
/// (voiding something already voided, say) are simply skipped: the point is
/// the mix of changes, not that each succeeds. Returns whether it did.
pub fn random_op(s: &mut Store, rng: &mut Rng) -> bool {
    let accounts: Vec<Account> = s.accounts().unwrap().into_iter().filter(|a| !a.archived).collect();
    let n = rng.below(100);
    match n {
        // A new account; codes come from a small pool so devices clash.
        0..=7 => {
            let code = format!("7{}", rng.below(4));
            let name = format!("Account {}", rng.next() % 1000);
            s.create_account(&NewAccount::new(name, AccountSubtype::Expense).code(code)).is_ok()
        }
        // The common case: money moves between two accounts.
        8..=39 => {
            let (Some(a), Some(b)) = (rng.pick(&accounts), rng.pick(&accounts)) else { return false };
            if a.id == b.id {
                return false;
            }
            let date = d(2026, 1 + rng.below(6) as u32, 1 + rng.below(28) as u32);
            let cents = 1 + rng.below(100_000) as i64;
            s.post_entry(&NewEntry::new(date, format!("e{}", rng.next() % 10_000)).line(EntryLine::debit(a.id, Cents(cents))).line(EntryLine::credit(b.id, Cents(cents)))).is_ok()
        }
        // Void an entry, possibly one another device is voiding too.
        40..=49 => {
            let entries = s.entries(d(2000, 1, 1), d(2099, 1, 1)).unwrap();
            let open: Vec<&Entry> = entries.iter().filter(|e| e.voided_by_id.is_none() && e.reverses_id.is_none()).collect();
            let Some(e) = rng.pick(&open) else { return false };
            s.void_entry(e.id, e.date).is_ok()
        }
        50..=54 => {
            let Some(a) = rng.pick(&accounts) else { return false };
            s.rename_account(a.id, &format!("Renamed {}", rng.next() % 1000)).is_ok()
        }
        55..=61 => {
            let kind = [ContactKind::Customer, ContactKind::Vendor, ContactKind::Both][rng.below(3)];
            let c = NewContact { kind, name: format!("Contact {}", rng.next() % 1000), company: None, email: None, phone: None, address: None, notes: None };
            s.create_contact(&c).is_ok()
        }
        62..=64 => {
            let contacts = s.contacts().unwrap();
            let Some(c) = rng.pick(&contacts) else { return false };
            let new = NewContact { kind: c.kind, name: format!("Edited {}", rng.next() % 1000), company: None, email: None, phone: None, address: None, notes: None };
            s.update_contact(c.id, &new).is_ok()
        }
        // An invoice: numbers come from a counter each device keeps, so they clash.
        65..=72 => {
            let contacts: Vec<Contact> = s.contacts().unwrap().into_iter().filter(|c| c.kind != ContactKind::Vendor).collect();
            let Some(c) = rng.pick(&contacts) else { return false };
            let id = invoice(s, c.id);
            if rng.chance(40) {
                let _ = s.finalize_invoice(id);
            } else if rng.chance(20) {
                let _ = s.delete_draft_invoice(id);
            }
            true
        }
        73..=77 => {
            let target = code(s, "6500");
            let name = format!("Rule {}", rng.next() % 1000);
            s.create_rule(&NewRule {
                name: name.clone(),
                priority: 100,
                op: MatchOp::Contains,
                pattern: name,
                direction: Direction::Any,
                amount_min: None,
                amount_max: None,
                bank_account_id: None,
                target_account_id: target,
                rename_to: None,
                auto_post: false,
                person_id: None,
            })
            .is_ok()
        }
        78..=80 => {
            let rules = s.rules().unwrap();
            let Some(r) = rng.pick(&rules) else { return false };
            s.delete_rule(r.id).is_ok()
        }
        81..=84 => s.set_pref("company.name", Some(&format!("Company {}", rng.next() % 100))).is_ok(),
        85..=87 => {
            let month = format!("2026-0{}", 1 + rng.below(3));
            let Some(a) = accounts.iter().find(|a| a.subtype == AccountSubtype::Expense) else { return false };
            let line = BudgetLine { target: BudgetTarget::Account(a.id), amount: Cents(100 * rng.below(1000) as i64), direction: Default::default(), tolerance_pct: 10 };
            s.set_budget_line(&month, &line).is_ok()
        }
        // A reconciliation started on a bank account (two devices can both start one).
        88..=90 => {
            let bank = code(s, "1000");
            if rng.chance(30)
                && let Ok(Some(open)) = s.active_reconciliation(bank)
            {
                return s.cancel_reconciliation(open.id).is_ok();
            }
            s.start_reconciliation(bank, d(2026, 6, 30), Cents(rng.below(10_000) as i64)).is_ok()
        }
        // Bank transactions arriving on more than one device with the same external id.
        91..=95 => {
            let bank = code(s, "1000");
            let t = FeedTransaction {
                external_id: format!("tx-{}", rng.below(6)),
                date: d(2026, 4, 1 + rng.below(28) as u32),
                amount: Cents(-(1 + rng.below(5000) as i64)),
                description: format!("SHOP {}", rng.below(5)),
                merchant: None,
                pending: false,
                category_hint: None,
            };
            s.import_feed(bank, &FeedBatch { added: vec![t], modified: vec![], removed: vec![] }).is_ok()
        }
        _ => {
            let Ok(inbox) = s.inbox(None) else { return false };
            let Some(item) = rng.pick(&inbox) else { return false };
            let target = code(s, "6500");
            if rng.chance(70) { s.accept(item.txn.id, target, None).is_ok() } else { s.exclude(item.txn.id).is_ok() }
        }
    }
}
