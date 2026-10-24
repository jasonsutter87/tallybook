//! The store's half of device sync: id ranges, the outbox, the clock, and
//! applying another device's changes. Several stores side by side, trading
//! changes directly (the `tally-sync` crate tests the same through an
//! encrypted transport). The design is `docs/SYNC.md`.
#![allow(clippy::inconsistent_digit_grouping, clippy::needless_range_loop)]

use chrono::NaiveDate;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tally_core::rules::{Direction, MatchOp};
use tally_core::{AccountId, AccountSubtype, Cents, EntryLine, NewEntry};
use tally_store::sync::*;
use tally_store::*;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

/// Later changes must have later clock readings; the clock counts milliseconds.
fn tick() {
    std::thread::sleep(Duration::from_millis(3));
}

fn temp_path(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("tally-sync-store-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(format!("{tag}-{}.tallybook", N.fetch_add(1, Ordering::SeqCst)))
}

fn code(s: &Store, c: &str) -> AccountId {
    s.account_by_code(c).unwrap().unwrap().id
}

/// The original device: a chart and two accounts, with sync not yet on.
fn original() -> Store {
    let mut s = Store::open_in_memory().unwrap();
    s.seed_default_chart().unwrap();
    s.create_account(&NewAccount::new("Checking", AccountSubtype::Bank).code("1000")).unwrap();
    s.create_account(&NewAccount::new("Visa", AccountSubtype::CreditCard).code("2000")).unwrap();
    s
}

/// A new device started from a snapshot of `source`, as Connect would number it.
fn join(source: &Store, device: u16) -> Store {
    let path = temp_path("snapshot");
    source.sync_snapshot_to(&path).unwrap();
    let mut s = Store::open(&path).unwrap();
    s.sync_become_device(device, 0).unwrap();
    s
}

/// `n` devices, the first the original, all with sync on and the same book.
fn devices(n: u16) -> Vec<Store> {
    let first = original();
    let mut all: Vec<Store> = (1..n).map(|i| join(&first, i)).collect();
    first.sync_enable().unwrap();
    all.insert(0, first);
    all
}

fn pending(s: &Store) -> Vec<RowChange> {
    s.sync_outbox_batch(1_000_000).unwrap().into_iter().map(|r| r.change).collect()
}

fn clear_outbox(s: &Store) {
    s.sync_outbox_done(i64::MAX).unwrap();
}

/// Every device hands every other its outbox, once.
fn sync_all(devs: &mut [Store]) -> Vec<ApplyReport> {
    let outs: Vec<Vec<RowChange>> = devs.iter().map(pending).collect();
    let mut reports = Vec::new();
    for (i, dev) in devs.iter_mut().enumerate() {
        for (j, changes) in outs.iter().enumerate() {
            if i != j {
                reports.push(dev.sync_apply(changes, None).unwrap());
            }
        }
    }
    devs.iter().for_each(clear_outbox);
    reports
}

fn assert_same_books(devs: &[Store]) {
    let first = devs[0].sync_dump().unwrap();
    for (i, other) in devs.iter().enumerate().skip(1) {
        let dump = other.sync_dump().unwrap();
        for (table, rows) in &first {
            assert_eq!(rows, &dump[table], "table {table} differs between device 0 and device {i}");
        }
    }
    let far = d(2099, 12, 31);
    let tb = devs[0].trial_balance(far).unwrap();
    assert!(tb.is_balanced());
    for other in &devs[1..] {
        assert_eq!(other.trial_balance(far).unwrap(), tb);
    }
}

fn post(s: &mut Store, date: NaiveDate, memo: &str, debit: AccountId, credit: AccountId, cents: i64) -> i64 {
    s.post_entry(&NewEntry::new(date, memo).line(EntryLine::debit(debit, Cents(cents))).line(EntryLine::credit(credit, Cents(cents)))).unwrap()
}

/// [`post`], naming the accounts by chart code.
fn postc(s: &mut Store, date: NaiveDate, memo: &str, debit: &str, credit: &str, cents: i64) -> i64 {
    let (debit, credit) = (code(s, debit), code(s, credit));
    post(s, date, memo, debit, credit, cents)
}

fn customer(s: &Store, name: &str) -> i64 {
    s.create_contact(&NewContact { kind: ContactKind::Customer, name: name.into(), company: None, email: None, phone: None, address: None, notes: None })
        .unwrap()
}

fn invoice(s: &mut Store, customer: i64) -> i64 {
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

fn rule_for(s: &Store, name: &str) -> i64 {
    s.create_rule(&NewRule {
        name: name.into(),
        priority: 100,
        op: MatchOp::Contains,
        pattern: name.into(),
        direction: Direction::Any,
        amount_min: None,
        amount_max: None,
        bank_account_id: None,
        target_account_id: code(s, "6500"),
        rename_to: None,
        auto_post: false,
        person_id: None,
    })
    .unwrap()
}

fn count(s: &Store, table: &str) -> usize {
    s.sync_dump().unwrap()[table].len()
}

fn rows(s: &Store, table: &str) -> Vec<Value> {
    s.sync_dump().unwrap()[table].iter().map(|r| serde_json::from_str(r).unwrap()).collect()
}

// ---------------------------------------------------------------- schema

#[test]
fn every_table_is_classified_and_every_column_has_a_trigger() {
    let s = Store::open_in_memory().unwrap();
    assert_eq!(s.sync_unclassified_tables().unwrap(), Vec::<String>::new(), "decide whether a new table syncs, in sync.rs");
    assert_eq!(s.sync_uncovered_columns().unwrap(), Vec::<String>::new());
}

#[test]
fn a_column_added_without_rebuilding_the_triggers_is_noticed() {
    let s = Store::open_in_memory().unwrap();
    s.debug_execute("ALTER TABLE accounts ADD COLUMN nickname TEXT").unwrap();
    assert_eq!(s.sync_uncovered_columns().unwrap(), vec!["accounts.nickname".to_string()]);
    s.debug_execute("CREATE TABLE sticky_notes (id INTEGER PRIMARY KEY)").unwrap();
    assert_eq!(s.sync_unclassified_tables().unwrap(), vec!["sticky_notes".to_string()]);
}

#[test]
fn a_book_that_never_syncs_records_nothing() {
    let mut s = original();
    postc(&mut s, d(2026, 1, 5), "x", "1000", "3000", 500);
    assert_eq!(s.sync_outbox_len().unwrap(), 0);
    assert!(!s.sync_enabled().unwrap());
}

#[test]
fn old_books_migrate_with_an_empty_outbox_and_keep_their_ids() {
    for fixture in ["business-v0.2.0", "household-v0.2.0"] {
        let from = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../tally-api/tests/fixtures/{fixture}.tallybook"));
        let path = temp_path(fixture);
        std::fs::copy(from, &path).unwrap();
        let mut s = Store::open(&path).unwrap();
        assert_eq!(s.sync_outbox_len().unwrap(), 0, "no backfill of old rows");
        assert_eq!(s.sync_device().unwrap(), 0);
        assert!(!s.sync_enabled().unwrap());
        assert_eq!(s.sync_uncovered_columns().unwrap(), Vec::<String>::new());
        let before: i64 = rows(&s, "journal_entries").iter().map(|e| e["id"].as_i64().unwrap()).max().unwrap();
        s.sync_enable().unwrap();
        let bank = s.accounts().unwrap().into_iter().find(|a| a.subtype == AccountSubtype::Bank).unwrap().id;
        let other = s.accounts().unwrap().into_iter().find(|a| a.id != bank && !a.archived && a.scope == s.account(bank).unwrap().scope).unwrap().id;
        let new_id = post(&mut s, d(2030, 1, 1), "after the upgrade", bank, other, 100);
        assert_eq!(new_id, before + 1, "device 0 numbers on from where the book left off");
        assert_eq!(s.sync_outbox_len().unwrap(), 3, "the entry and its two lines");
    }
}

// ---------------------------------------------------------------- ids

#[test]
fn each_device_numbers_rows_from_its_own_range() {
    let devs = devices(3);
    for (n, dev) in devs.iter().enumerate() {
        let id = dev.create_account(&NewAccount::new(format!("Account {n}"), AccountSubtype::Expense)).unwrap();
        let (lo, hi) = id_range(n as i64);
        assert!((lo..=hi).contains(&id), "device {n} made id {id}");
        assert_eq!(dev.sync_device().unwrap(), n as u16);
    }
    assert!(id_range(2).0 >= 2 << 40);
}

#[test]
fn ids_never_collide_across_devices_and_tables() {
    let mut devs = devices(3);
    for round in 0..4 {
        for (n, dev) in devs.iter_mut().enumerate() {
            let a = dev.create_account(&NewAccount::new(format!("A{n}-{round}"), AccountSubtype::Expense).code(format!("9{n}{round}"))).unwrap();
            let bank = code(dev, "1000");
            post(dev, d(2026, 2, 1 + round), &format!("e{n}{round}"), a, bank, 100 * (n as i64 + 1));
            let c = customer(dev, &format!("C{n}{round}"));
            invoice(dev, c);
            dev.create_person(&NewPerson { name: format!("P{n}{round}"), color: None }).unwrap();
            rule_for(dev, &format!("R{n}{round}"));
        }
    }
    let made = [("accounts", 12), ("journal_entries", 12), ("contacts", 12), ("invoices", 12), ("invoice_lines", 12), ("people", 12), ("rules", 12)];
    let before: Vec<usize> = made.iter().map(|(t, _)| count(&devs[0], t)).collect();
    sync_all(&mut devs);
    assert_same_books(&devs);
    for ((table, new), before) in made.iter().zip(before) {
        // Device 0 already had its own 4 rounds; the others' 8 arrive intact: nothing overwrote anything.
        assert_eq!(count(&devs[0], table), before + new - 4, "{table}: a colliding id would have merged two rows into one");
    }
    // Every id is in the range of the device that made it (or the shared start).
    for table in id_tables() {
        for r in rows(&devs[0], table) {
            let id = r["id"].as_i64().unwrap();
            assert!(id < 1 << 53);
        }
    }
}

// ---------------------------------------------------------------- the outbox

#[test]
fn changes_land_in_the_outbox_in_order_with_whole_rows() {
    let mut s = original();
    s.sync_enable().unwrap();
    let id = postc(&mut s, d(2026, 1, 5), "Sale", "1000", "4000", 1234);
    s.rename_account(code(&s, "1000"), "Main checking").unwrap();
    let out = s.sync_outbox_batch(100).unwrap();
    let tables: Vec<(&str, ChangeOp)> = out.iter().map(|r| (r.change.table.as_str(), r.change.op)).collect();
    assert_eq!(
        tables,
        vec![
            ("journal_entries", ChangeOp::Insert),
            ("journal_lines", ChangeOp::Insert),
            ("journal_lines", ChangeOp::Insert),
            ("accounts", ChangeOp::Update),
        ]
    );
    assert_eq!(out[0].change.pk, vec![json!(id)]);
    assert_eq!(out[0].change.row.as_ref().unwrap()["memo"], "Sale");
    assert_eq!(out[1].change.row.as_ref().unwrap()["amount"], 1234);
    assert_eq!(out[3].change.row.as_ref().unwrap()["name"], "Main checking");
    let clocks: Vec<&str> = out.iter().map(|r| r.change.hlc.as_str()).collect();
    assert!(clocks.windows(2).all(|w| w[0] < w[1]), "the clock only goes up: {clocks:?}");
}

#[test]
fn deletes_are_recorded_and_this_devices_own_settings_are_not() {
    let mut s = original();
    s.sync_enable().unwrap();
    let rule = rule_for(&s, "Zoom");
    s.delete_rule(rule).unwrap();
    s.set_pref("cloud.folder", Some("/Users/me/Dropbox")).unwrap();
    s.set_pref("company.name", Some("Acme")).unwrap();
    let out = pending(&s);
    assert!(out.iter().any(|c| c.table == "rules" && c.op == ChangeOp::Delete && c.row.is_none() && c.pk == vec![json!(rule)]));
    let keys: Vec<&str> = out.iter().filter(|c| c.table == "settings").map(|c| c.pk[0].as_str().unwrap()).collect();
    assert_eq!(keys, vec!["pref.company.name"], "the cloud backup folder belongs to this device");
}

#[test]
fn a_batch_never_ends_inside_an_entry() {
    let mut s = original();
    s.sync_enable().unwrap();
    postc(&mut s, d(2026, 1, 5), "a", "1000", "4000", 100);
    postc(&mut s, d(2026, 1, 6), "b", "1000", "4000", 100);
    // Asking for one row gets the entry and both its lines.
    let first = s.sync_outbox_batch(1).unwrap();
    assert_eq!(first.len(), 3);
    s.sync_outbox_done(first.last().unwrap().seq).unwrap();
    assert_eq!(s.sync_outbox_batch(1).unwrap().len(), 3);
}

#[test]
fn the_clock_never_goes_backwards_and_follows_a_device_that_is_ahead() {
    let mut devs = devices(2);
    let mut last = devs[0].sync_clock().unwrap();
    for i in 0..50 {
        postc(&mut devs[0], d(2026, 1, 1), &format!("{i}"), "1000", "4000", 1);
        let now = devs[0].sync_clock().unwrap();
        assert!(now > last);
        last = now;
    }
    // Device 1's clock is a day fast.
    let future = Hlc { wall_ms: last.wall_ms + 86_400_000, counter: 7, device: 1 };
    let change = RowChange {
        table: "settings".into(),
        pk: vec![json!("pref.company.name")],
        op: ChangeOp::Insert,
        row: Some(json!({"key": "pref.company.name", "value": "Fast"}).as_object().unwrap().clone()),
        hlc: future.to_string(),
    };
    devs[0].sync_apply(&[change], None).unwrap();
    let after = devs[0].sync_clock().unwrap();
    assert!(after >= Hlc { device: 0, ..future }, "the clock caught up with what it saw");
    postc(&mut devs[0], d(2026, 1, 1), "later", "1000", "4000", 1);
    assert!(devs[0].sync_clock().unwrap() > after);
}

// ---------------------------------------------------------------- merging

#[test]
fn offline_edits_on_each_device_end_with_identical_books() {
    let mut devs = devices(3);
    for n in 0..3usize {
        let dev = &mut devs[n];
        let bank = code(dev, "1000");
        let acct = dev.create_account(&NewAccount::new(format!("Project {n}"), AccountSubtype::Expense).code(format!("70{n}"))).unwrap();
        post(dev, d(2026, 4, 1 + n as u32), &format!("spend {n}"), acct, bank, 5_00 * (n as i64 + 1));
        let c = customer(dev, &format!("Customer {n}"));
        invoice(dev, c);
        dev.set_pref("company.name", Some(&format!("Name {n}"))).unwrap();
        tick();
    }
    sync_all(&mut devs);
    assert_same_books(&devs);
    assert_eq!(rows(&devs[0], "settings").iter().filter(|r| r["key"] == "pref.company.name").count(), 1);
    assert_eq!(rows(&devs[1], "settings").iter().find(|r| r["key"] == "pref.company.name").unwrap()["value"], "Name 2", "the last write wins");
    // Syncing again changes nothing.
    sync_all(&mut devs);
    assert_same_books(&devs);
}

#[test]
fn the_same_edits_applied_in_any_order_give_the_same_book() {
    let mut devs = devices(3);
    let account = code(&devs[0], "6500");
    for n in 0..3usize {
        let dev = &mut devs[n];
        dev.rename_account(account, &format!("Software {n}")).unwrap();
        tick();
        let c = customer(dev, "Same name");
        dev.create_account(&NewAccount::new("Twin", AccountSubtype::Expense).code("7777")).unwrap();
        invoice(dev, c);
        post(dev, d(2026, 5, 1), "x", account, code(dev, "1000"), 10 * (n as i64 + 1));
        tick();
    }
    let outs: Vec<Vec<RowChange>> = devs.iter().map(pending).collect();
    let base = original();
    let results: Vec<_> = [[0, 1, 2], [2, 1, 0], [1, 2, 0], [2, 0, 1]]
        .iter()
        .map(|order| {
            let mut r = join(&base, 9);
            for &i in order {
                r.sync_apply(&outs[i], None).unwrap();
            }
            r
        })
        .collect();
    assert_same_books(&results);
    // And it is what the devices themselves end with.
    sync_all(&mut devs);
    assert_same_books(&devs);
    assert_eq!(devs[0].sync_dump().unwrap()["accounts"].len(), results[0].sync_dump().unwrap()["accounts"].len());
}

#[test]
fn a_void_on_two_devices_nets_to_one_void() {
    let mut devs = devices(2);
    let (bank, sales) = (code(&devs[0], "1000"), code(&devs[0], "4000"));
    let entry = post(&mut devs[0], d(2026, 6, 1), "Sale", bank, sales, 250_00);
    sync_all(&mut devs);
    let start = devs[0].account_balance(bank, d(2026, 12, 31)).unwrap();
    assert_eq!(start, Cents(250_00));
    // Offline, each voids the same entry.
    devs[0].void_entry(entry, d(2026, 6, 2)).unwrap();
    tick();
    devs[1].void_entry(entry, d(2026, 6, 3)).unwrap();
    let reports = sync_all(&mut devs);
    assert!(reports.iter().map(|r| r.voids_merged).sum::<usize>() >= 2, "both devices noticed");
    assert_same_books(&devs);
    for dev in &devs {
        assert_eq!(dev.account_balance(bank, d(2026, 12, 31)).unwrap(), Cents(0), "voided once, not twice");
        assert_eq!(dev.account_balance(sales, d(2026, 12, 31)).unwrap(), Cents(0));
        let e = dev.entry(entry).unwrap();
        assert!(e.voided_by_id.is_some());
    }
    // The first void (device 0's) stands; the other is itself reversed.
    let winner = devs[0].entry(entry).unwrap().voided_by_id.unwrap();
    let voids: Vec<Value> = rows(&devs[0], "journal_entries").into_iter().filter(|e| e["source"] == "void").collect();
    assert_eq!(voids.len(), 3, "two voids and the reversal that cancels the second: {voids:?}");
    assert!(winner < 1 << 41, "device 0 voided first");
    // A third and fourth sync is stable.
    sync_all(&mut devs);
    assert_same_books(&devs);
    assert_eq!(rows(&devs[1], "journal_entries").len(), rows(&devs[0], "journal_entries").len());
}

#[test]
fn three_devices_voiding_the_same_entry_still_net_to_one_void() {
    let mut devs = devices(3);
    let (bank, sales) = (code(&devs[0], "1000"), code(&devs[0], "4000"));
    let entry = post(&mut devs[0], d(2026, 6, 1), "Sale", bank, sales, 99_99);
    sync_all(&mut devs);
    for (i, dev) in devs.iter_mut().enumerate().rev() {
        dev.void_entry(entry, d(2026, 6, 2 + i as u32)).unwrap();
        tick();
    }
    sync_all(&mut devs);
    assert_same_books(&devs);
    for dev in &devs {
        assert_eq!(dev.account_balance(bank, d(2026, 12, 31)).unwrap(), Cents(0));
    }
    // Applied one device's changes at a time, in another order, to a fresh copy.
    let mut late = join(&devs[0], 7);
    late.sync_apply(&[], None).unwrap();
    assert_eq!(late.account_balance(bank, d(2026, 12, 31)).unwrap(), Cents(0));
}

#[test]
fn a_delete_wins_over_an_older_edit_and_loses_to_a_newer_one() {
    let mut devs = devices(2);
    let (old_rule, new_rule) = (rule_for(&devs[0], "Old"), rule_for(&devs[0], "New"));
    sync_all(&mut devs);
    // Edit first, delete after: the delete wins.
    let edit = |dev: &Store, id: i64, name: &str| {
        let mut r = NewRule {
            name: name.into(),
            priority: 100,
            op: MatchOp::Contains,
            pattern: "x".into(),
            direction: Direction::Any,
            amount_min: None,
            amount_max: None,
            bank_account_id: None,
            target_account_id: code(dev, "6500"),
            rename_to: None,
            auto_post: false,
            person_id: None,
        };
        r.name = name.into();
        dev.update_rule(id, &r).unwrap();
    };
    edit(&devs[1], old_rule, "edited early");
    tick();
    devs[0].delete_rule(old_rule).unwrap();
    // Delete first, edit after: the edit survives.
    tick();
    devs[0].delete_rule(new_rule).unwrap();
    tick();
    edit(&devs[1], new_rule, "edited late");
    sync_all(&mut devs);
    assert_same_books(&devs);
    let names: Vec<String> = rows(&devs[0], "rules").iter().map(|r| r["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(names, vec!["edited late".to_string()]);
}

#[test]
fn clashing_account_codes_and_invoice_numbers_are_renamed_the_same_way_everywhere() {
    let mut devs = devices(3);
    for n in 0..3usize {
        let dev = &mut devs[n];
        dev.create_account(&NewAccount::new(format!("Mine {n}"), AccountSubtype::Expense).code("7100")).unwrap();
        let c = customer(dev, &format!("Cust {n}"));
        invoice(dev, c);
        tick();
    }
    let reports = sync_all(&mut devs);
    assert_same_books(&devs);
    assert!(reports.iter().any(|r| !r.renamed.is_empty()), "the person is told");
    let mut codes: Vec<String> = rows(&devs[0], "accounts").iter().filter_map(|a| a["code"].as_str()).filter(|c| c.starts_with("7100")).map(String::from).collect();
    codes.sort();
    assert_eq!(codes, vec!["7100", "7100-2", "7100-3"]);
    let mut numbers: Vec<String> = rows(&devs[2], "invoices").iter().map(|i| i["number"].as_str().unwrap().to_string()).collect();
    numbers.sort();
    let first = numbers[0].clone();
    assert_eq!(numbers, vec![first.clone(), format!("{first}-2"), format!("{first}-3")]);
    // The earliest-made account keeps the plain code on every device.
    for dev in &devs {
        let kept = dev.account_by_code("7100").unwrap().unwrap();
        assert_eq!(kept.name, "Mine 0");
    }
}

#[test]
fn applying_the_same_changes_twice_changes_nothing() {
    let mut devs = devices(2);
    let acct = devs[1].create_account(&NewAccount::new("Once", AccountSubtype::Expense).code("7300")).unwrap();
    let bank1 = code(&devs[1], "1000");
    post(&mut devs[1], d(2026, 7, 1), "x", acct, bank1, 400);
    let changes = pending(&devs[1]);
    let first = devs[0].sync_apply(&changes, None).unwrap();
    assert!(first.applied > 0);
    let dump = devs[0].sync_dump().unwrap();
    let second = devs[0].sync_apply(&changes, None).unwrap();
    assert_eq!(second.applied, 0);
    assert_eq!(devs[0].sync_dump().unwrap(), dump);
}

#[test]
fn applying_does_not_echo_back_into_the_outbox() {
    let mut devs = devices(2);
    let acct = devs[1].create_account(&NewAccount::new("Quiet", AccountSubtype::Expense)).unwrap();
    let changes = pending(&devs[1]);
    devs[0].sync_apply(&changes, Some(5)).unwrap();
    assert_eq!(devs[0].sync_outbox_len().unwrap(), 0);
    assert_eq!(devs[0].sync_last_seq().unwrap(), 5);
    assert_eq!(devs[0].account(acct).unwrap().name, "Quiet");
}

#[test]
fn local_settings_do_not_travel() {
    let mut devs = devices(2);
    let change = RowChange {
        table: "settings".into(),
        pk: vec![json!("pref.cloud.folder")],
        op: ChangeOp::Insert,
        row: Some(json!({"key": "pref.cloud.folder", "value": "/elsewhere"}).as_object().unwrap().clone()),
        hlc: Hlc { wall_ms: 1, counter: 0, device: 1 }.to_string(),
    };
    devs[0].sync_apply(&[change], None).unwrap();
    assert_eq!(devs[0].pref("cloud.folder").unwrap(), None);
}

// ---------------------------------------------------------------- safety

#[test]
fn changes_that_would_unbalance_the_books_are_refused_whole() {
    let mut devs = devices(2);
    let (bank, sales) = (code(&devs[0], "1000"), code(&devs[0], "4000"));
    let ok = json!({"id": 5i64 << 40, "date": "2026-01-01", "memo": "fine", "source": "manual", "reverses_id": null, "voided_by_id": null, "created_at": "2026-01-01 00:00:00", "payee_id": null, "person_id": null, "goal_id": null});
    let line = |id: i64, amount: i64| {
        json!({"id": id, "entry_id": 5i64 << 40, "account_id": bank, "amount": amount, "memo": null, "cleared": "uncleared", "reconciliation_id": null})
    };
    let hlc = |c: u32| Hlc { wall_ms: 10, counter: c, device: 5 }.to_string();
    let change = |table: &str, id: i64, row: Value, c: u32| RowChange {
        table: table.into(),
        pk: vec![json!(id)],
        op: ChangeOp::Insert,
        row: Some(row.as_object().unwrap().clone()),
        hlc: hlc(c),
    };
    let before = devs[0].sync_dump().unwrap();
    let bad = vec![
        change("accounts", (5i64 << 40) + 1, json!({"id": (5i64 << 40) + 1, "code": "9999", "name": "Harmless", "subtype": "expense", "parent_id": null, "archived": 0, "created_at": "2026-01-01 00:00:00", "scope": "business", "flex": null}), 0),
        change("journal_entries", 5i64 << 40, ok.clone(), 1),
        change("journal_lines", 5i64 << 40, line(5i64 << 40, 100), 2),
    ];
    let err = devs[0].sync_apply(&bad, Some(9)).unwrap_err();
    assert!(err.to_string().contains("out of balance"), "{err}");
    assert_eq!(devs[0].sync_dump().unwrap(), before, "nothing was applied, not even the harmless account");
    assert_eq!(devs[0].sync_last_seq().unwrap(), 0);
    // And the store is still usable: no stuck "applying" flag letting edits escape the outbox.
    let acct = devs[0].create_account(&NewAccount::new("After", AccountSubtype::Expense)).unwrap();
    assert!(pending(&devs[0]).iter().any(|c| c.table == "accounts" && c.pk == vec![json!(acct)]));
    // A balanced pair is fine.
    let mut good = bad.clone();
    good.push(change("journal_lines", (5i64 << 40) + 1, json!({"id": (5i64 << 40) + 1, "entry_id": 5i64 << 40, "account_id": sales, "amount": -100, "memo": null, "cleared": "uncleared", "reconciliation_id": null}), 3));
    good[2].row.as_mut().unwrap().insert("amount".into(), json!(100));
    devs[0].sync_apply(&good, Some(9)).unwrap();
    assert_eq!(devs[0].entry(5i64 << 40).unwrap().lines.len(), 2);
}

#[test]
fn the_ledger_still_refuses_local_edits_with_sync_on() {
    let mut devs = devices(2);
    let (bank, sales) = (code(&devs[0], "1000"), code(&devs[0], "4000"));
    let entry = post(&mut devs[0], d(2026, 1, 5), "x", bank, sales, 100);
    sync_all(&mut devs);
    for sql in [
        format!("UPDATE journal_entries SET date = '2020-01-01' WHERE id = {entry}"),
        format!("UPDATE journal_entries SET reverses_id = {entry} WHERE id = {entry}"),
        format!("DELETE FROM journal_entries WHERE id = {entry}"),
        format!("UPDATE journal_lines SET amount = 5 WHERE entry_id = {entry}"),
        format!("DELETE FROM journal_lines WHERE entry_id = {entry}"),
    ] {
        for dev in &devs {
            assert!(dev.debug_execute(&sql).is_err(), "{sql} should be refused");
        }
    }
    // A void can still be set once, and not replaced, by anyone but sync.
    devs[0].void_entry(entry, d(2026, 1, 6)).unwrap();
    assert!(devs[0].debug_execute(&format!("UPDATE journal_entries SET voided_by_id = 999 WHERE id = {entry}")).is_err());
    assert!(devs[0].debug_execute(&format!("UPDATE journal_entries SET voided_by_id = NULL WHERE id = {entry}")).is_err());
    assert!(devs[0].sync_outbox_len().unwrap() > 0);
}

#[test]
fn a_change_to_a_table_or_column_this_version_does_not_know_is_refused() {
    let mut s = original();
    let mk = |table: &str, row: Value| RowChange {
        table: table.into(),
        pk: vec![json!(1)],
        op: ChangeOp::Insert,
        row: Some(row.as_object().unwrap().clone()),
        hlc: Hlc { wall_ms: 1, counter: 0, device: 1 }.to_string(),
    };
    assert!(s.sync_apply(&[mk("holograms", json!({"id": 1}))], None).is_err());
    assert!(s.sync_apply(&[mk("people", json!({"id": 1, "name": "A", "telepathy": 1}))], None).is_err());
}

#[test]
fn a_new_device_from_a_snapshot_plus_later_changes_matches_the_source() {
    let mut devs = devices(2);
    let (bank, sales) = (code(&devs[0], "1000"), code(&devs[0], "4000"));
    post(&mut devs[0], d(2026, 1, 5), "before the snapshot", bank, sales, 700);
    clear_outbox(&devs[0]);
    let mut newcomer = join(&devs[0], 4);
    assert_eq!(newcomer.sync_dump().unwrap(), devs[0].sync_dump().unwrap(), "a snapshot is the whole book");
    // The source keeps going; the newcomer catches up from the changes.
    post(&mut devs[0], d(2026, 1, 6), "after", bank, sales, 300);
    devs[0].rename_account(bank, "Renamed").unwrap();
    let changes = pending(&devs[0]);
    newcomer.sync_apply(&changes, Some(3)).unwrap();
    assert_eq!(newcomer.sync_dump().unwrap(), devs[0].sync_dump().unwrap());
    assert_eq!(newcomer.sync_device().unwrap(), 4);
    // Its own ids come from its own range.
    let id = newcomer.create_account(&NewAccount::new("Mine", AccountSubtype::Expense)).unwrap();
    assert!(id >= 4 << 40);
}
