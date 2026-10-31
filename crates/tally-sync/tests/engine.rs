//! Devices trading changes through the (in-memory) server: the whole path
//! from outbox to encrypted batch to applied row, and back to a new device
//! from a snapshot.
#![allow(clippy::inconsistent_digit_grouping, clippy::needless_range_loop)]

mod common;

use common::*;
use tally_core::rules::{Direction, MatchOp};
use tally_core::{AccountSubtype, Cents};
use tally_store::sync::id_range;
use tally_store::*;
use tally_sync::*;

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

fn rows(s: &Store, table: &str) -> Vec<serde_json::Value> {
    s.sync_dump().unwrap()[table].iter().map(|r| serde_json::from_str(r).unwrap()).collect()
}

#[test]
fn offline_edits_on_each_device_end_with_identical_books() {
    let mut w = World::new(3);
    for n in 0..3usize {
        let dev = &mut w.devs[n].store;
        let bank = code(dev, "1000");
        let acct = dev.create_account(&NewAccount::new(format!("Project {n}"), AccountSubtype::Expense).code(format!("70{n}"))).unwrap();
        post(dev, d(2026, 4, 1 + n as u32), &format!("spend {n}"), acct, bank, 5_00 * (n as i64 + 1));
        let c = customer(dev, &format!("Customer {n}"));
        invoice(dev, c);
        dev.set_pref("company.name", Some(&format!("Name {n}"))).unwrap();
        tick();
    }
    w.sync_everyone();
    w.assert_converged();
    assert_eq!(rows(&w.devs[0].store, "accounts").iter().filter(|a| a["code"].as_str().is_some_and(|c| c.len() == 3 && c.starts_with("70"))).count(), 3);
    assert_eq!(rows(&w.devs[1].store, "settings").iter().find(|r| r["key"] == "pref.company.name").unwrap()["value"], "Name 2");
    w.finish();
}

#[test]
fn syncing_again_does_nothing() {
    let mut w = World::new(2);
    let (bank, sales) = (code(&w.devs[0].store, "1000"), code(&w.devs[0].store, "4000"));
    post(&mut w.devs[0].store, d(2026, 1, 5), "x", bank, sales, 100);
    w.sync_everyone();
    let before = w.devs[1].store.sync_dump().unwrap();
    let again = w.sync_in_order(&[0, 1]);
    for r in again {
        assert_eq!((r.batches_pushed, r.changes_pushed, r.applied.applied), (0, 0, 0), "{r:?}");
    }
    assert_eq!(w.devs[1].store.sync_dump().unwrap(), before);
    w.finish();
}

#[test]
fn the_same_edits_arriving_in_any_order_give_the_same_book() {
    let mut w = World::new(3);
    let account = code(&w.devs[0].store, "6500");
    for n in 0..3usize {
        let dev = &mut w.devs[n].store;
        dev.rename_account(account, &format!("Software {n}")).unwrap();
        tick();
        let c = customer(dev, "Same name");
        dev.create_account(&NewAccount::new("Twin", AccountSubtype::Expense).code("7777")).unwrap();
        invoice(dev, c);
        let bank = code(dev, "1000");
        post(dev, d(2026, 5, 1), "x", account, bank, 10 * (n as i64 + 1));
        tick();
    }
    for dev in &mut w.devs {
        dev.push(&w.key).unwrap();
    }
    let book = w.key.book_id.clone();
    let batches = w.server.batches(&book);
    assert_eq!(batches.len(), 3);
    let snapshot = w.devs[0].net.get_snapshot(&book).unwrap().unwrap();
    // The same three batches, pushed to fresh servers in every order, pulled by fresh devices.
    let mut results = Vec::new();
    for order in [[0, 1, 2], [2, 1, 0], [1, 2, 0], [0, 2, 1], [2, 0, 1], [1, 0, 2]] {
        let server = MemoryServer::new();
        let mut net = server.connect();
        net.put_snapshot(&book, snapshot.0, snapshot.1.clone()).unwrap();
        // The three devices already have numbers 0 to 2 on the real server; the newcomer must not be mistaken for one.
        for _ in 0..3 {
            net.join(&book).unwrap();
        }
        for &i in &order {
            net.push_batch(&book, batches[i].device, &batches[i].batch_id, batches[i].data.clone()).unwrap();
        }
        let dir = temp_dir();
        let mut store = join_from_snapshot(&mut net, &w.key, &dir.join("late.tallybook")).unwrap();
        sync_once(&mut store, &mut net, &w.key).unwrap();
        results.push((store, dir));
    }
    let stores: Vec<&Store> = results.iter().map(|(s, _)| s).collect();
    assert_same_books(&stores);
    // They also match what the three devices themselves end up with.
    w.sync_everyone();
    w.assert_converged();
    assert_eq!(w.devs[0].store.sync_dump().unwrap()["accounts"], stores[0].sync_dump().unwrap()["accounts"]);
    for (_, dir) in results {
        let _ = std::fs::remove_dir_all(dir);
    }
    w.finish();
}

#[test]
fn a_void_on_two_devices_nets_to_one_void() {
    let mut w = World::new(2);
    let (bank, sales) = (code(&w.devs[0].store, "1000"), code(&w.devs[0].store, "4000"));
    let entry = post(&mut w.devs[0].store, d(2026, 6, 1), "Sale", bank, sales, 250_00);
    w.sync_everyone();
    w.devs[0].store.void_entry(entry, d(2026, 6, 2)).unwrap();
    tick();
    w.devs[1].store.void_entry(entry, d(2026, 6, 3)).unwrap();
    let reports = w.sync_everyone();
    assert!(reports.iter().map(|r| r.applied.voids_merged).sum::<usize>() >= 2);
    w.assert_converged();
    for dev in &w.devs {
        assert_eq!(dev.store.account_balance(bank, d(2026, 12, 31)).unwrap(), Cents(0), "voided once, not twice");
        assert_eq!(dev.store.account_balance(sales, d(2026, 12, 31)).unwrap(), Cents(0));
    }
    // Device 0 voided first, so its void stands everywhere.
    let winner = w.devs[1].store.entry(entry).unwrap().voided_by_id.unwrap();
    assert!(winner < 1 << 40);
    w.finish();
}

#[test]
fn delete_versus_edit_goes_by_which_is_newer() {
    let mut w = World::new(2);
    let (a, b) = (rule_for(&w.devs[0].store, "Old"), rule_for(&w.devs[0].store, "New"));
    w.sync_everyone();
    let edit = |s: &Store, id: i64, name: &str| {
        s.update_rule(
            id,
            &NewRule {
                name: name.into(),
                priority: 100,
                op: MatchOp::Contains,
                pattern: "x".into(),
                direction: Direction::Any,
                amount_min: None,
                amount_max: None,
                bank_account_id: None,
                target_account_id: code(s, "6500"),
                rename_to: None,
                auto_post: false,
                person_id: None,
            },
        )
        .unwrap();
    };
    edit(&w.devs[1].store, a, "edited early");
    tick();
    w.devs[0].store.delete_rule(a).unwrap();
    tick();
    w.devs[0].store.delete_rule(b).unwrap();
    tick();
    edit(&w.devs[1].store, b, "edited late");
    w.sync_everyone();
    w.assert_converged();
    let names: Vec<String> = rows(&w.devs[0].store, "rules").iter().map(|r| r["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(names, vec!["edited late".to_string()], "the older edit lost to the delete; the newer edit beat its delete");
    w.finish();
}

#[test]
fn clashing_codes_and_invoice_numbers_are_renamed_and_reported() {
    let mut w = World::new(3);
    for n in 0..3usize {
        let dev = &mut w.devs[n].store;
        dev.create_account(&NewAccount::new(format!("Mine {n}"), AccountSubtype::Expense).code("7100")).unwrap();
        let c = customer(dev, &format!("Cust {n}"));
        invoice(dev, c);
        tick();
    }
    let reports = w.sync_everyone();
    w.assert_converged();
    let renamed: Vec<&tally_store::sync::Renamed> = reports.iter().flat_map(|r| &r.applied.renamed).collect();
    assert!(renamed.iter().any(|r| r.table == "accounts" && r.from == "7100" && r.to == "7100-2"), "{renamed:?}");
    assert!(renamed.iter().any(|r| r.table == "invoices"));
    let mut codes: Vec<String> = rows(&w.devs[0].store, "accounts").iter().filter_map(|a| a["code"].as_str()).filter(|c| c.starts_with("7100")).map(String::from).collect();
    codes.sort();
    assert_eq!(codes, vec!["7100", "7100-2", "7100-3"]);
    for dev in &w.devs {
        assert_eq!(dev.store.account_by_code("7100").unwrap().unwrap().name, "Mine 0", "the earliest keeps the plain code on every device");
    }
    w.finish();
}

#[test]
fn a_clash_that_goes_away_leaves_no_suffix_behind() {
    let mut w = World::new(2);
    let _mine = w.devs[0].store.create_account(&NewAccount::new("Mine", AccountSubtype::Expense).code("7200")).unwrap();
    tick();
    let theirs = w.devs[1].store.create_account(&NewAccount::new("Theirs", AccountSubtype::Expense).code("7200")).unwrap();
    w.sync_everyone();
    w.assert_converged();
    assert_eq!(w.devs[0].store.account(theirs).unwrap().code.as_deref(), Some("7200-2"));
    // The device that made the first one never used it; there is no delete for accounts, but a
    // draft invoice can go: the same holds for invoice numbers.
    let c = customer(&w.devs[0].store, "C");
    let first = invoice(&mut w.devs[0].store, c);
    tick();
    let c2 = customer(&w.devs[1].store, "C2");
    let second = invoice(&mut w.devs[1].store, c2);
    w.sync_everyone();
    let shown = |s: &Store, id: i64| rows(s, "invoices").into_iter().find(|i| i["id"] == id).unwrap()["number"].as_str().unwrap().to_string();
    let plain = shown(&w.devs[0].store, first);
    assert_eq!(shown(&w.devs[1].store, second), format!("{plain}-2"));
    w.devs[0].store.delete_draft_invoice(first).unwrap();
    w.sync_everyone();
    w.assert_converged();
    assert_eq!(shown(&w.devs[0].store, second), plain, "with the clash gone the second invoice has its own number back");
    w.finish();
}

#[test]
fn ids_never_collide() {
    let mut w = World::new(3);
    for round in 0..4 {
        for n in 0..3usize {
            let dev = &mut w.devs[n].store;
            let a = dev.create_account(&NewAccount::new(format!("A{n}-{round}"), AccountSubtype::Expense).code(format!("9{n}{round}"))).unwrap();
            let (lo, hi) = id_range(n as i64);
            assert!((lo..=hi).contains(&a));
            let bank = code(dev, "1000");
            post(dev, d(2026, 2, 1 + round), &format!("e{n}{round}"), a, bank, 100 * (n as i64 + 1));
            let c = customer(dev, &format!("C{n}{round}"));
            invoice(dev, c);
        }
    }
    let before = rows(&w.devs[0].store, "journal_entries").len();
    w.sync_everyone();
    w.assert_converged();
    assert_eq!(rows(&w.devs[0].store, "journal_entries").len(), before + 8, "no entry replaced another");
    assert_eq!(rows(&w.devs[0].store, "invoices").len(), 12);
    assert_eq!(rows(&w.devs[0].store, "contacts").len(), 12);
    w.finish();
}

#[test]
fn a_big_outbox_goes_in_several_batches_that_never_split_an_entry() {
    let mut w = World::new(2);
    let (bank, sales) = (code(&w.devs[0].store, "1000"), code(&w.devs[0].store, "4000"));
    for i in 0..400 {
        post(&mut w.devs[0].store, d(2026, 3, 1 + (i % 28)), &format!("sale {i}"), bank, sales, 1 + i as i64);
    }
    let report = w.devs[0].sync(&w.key).unwrap();
    assert!(report.batches_pushed >= 2 && report.changes_pushed == 1200, "{report:?}");
    let report = w.devs[1].sync(&w.key).unwrap();
    assert_eq!(report.batches_pulled, w.server.batches(&w.key.book_id).len());
    w.sync_everyone();
    w.assert_converged();
    w.finish();
}

// ---------------------------------------------------------------- crashes and retries

#[test]
fn a_failure_anywhere_followed_by_a_retry_converges() {
    for fault in [Fault::RequestLost, Fault::ResponseLost] {
        for at in 0..8 {
            let mut w = World::new(2);
            let (bank, sales) = (code(&w.devs[0].store, "1000"), code(&w.devs[0].store, "4000"));
            for i in 0..3 {
                post(&mut w.devs[0].store, d(2026, 1, 1 + i), "a", bank, sales, 100 + i as i64);
            }
            post(&mut w.devs[1].store, d(2026, 1, 9), "b", bank, sales, 7);
            // Device 0 fails mid-sync, device 1 completes, device 0 retries.
            w.devs[0].net.fail(at, fault);
            let first = w.devs[0].sync(&w.key);
            w.devs[0].net.clear_fault();
            w.devs[1].sync(&w.key).unwrap();
            if first.is_err() {
                // A retry may need more than one go if another call was lost; none are.
                w.devs[0].sync(&w.key).unwrap();
            }
            w.sync_everyone();
            w.assert_converged();
            // The server holds each batch once, however many times it was sent.
            let batches = w.server.batches(&w.key.book_id);
            let ids: std::collections::BTreeSet<_> = batches.iter().map(|b| (b.device, b.batch_id.clone())).collect();
            assert_eq!(ids.len(), batches.len(), "{fault:?} at call {at}: a batch was stored twice");
            assert_eq!(rows(&w.devs[1].store, "journal_entries").len(), 4, "{fault:?} at call {at}");
            w.finish();
        }
    }
}

#[test]
fn a_crash_after_pushing_but_before_forgetting_sends_nothing_new() {
    let mut w = World::new(2);
    let (bank, sales) = (code(&w.devs[0].store, "1000"), code(&w.devs[0].store, "4000"));
    post(&mut w.devs[0].store, d(2026, 1, 1), "a", bank, sales, 5);
    // The push reaches the server but the answer is lost, so the outbox isn't cleared.
    w.devs[0].net.fail(0, Fault::ResponseLost);
    assert!(w.devs[0].push(&w.key).is_err());
    assert!(w.devs[0].store.sync_outbox_len().unwrap() > 0);
    // Retrying pushes the same batch under the same id: the server keeps one.
    w.sync_everyone();
    assert_eq!(w.server.batches(&w.key.book_id).len(), 1);
    w.assert_converged();
    w.finish();
}

#[test]
fn a_batch_that_would_unbalance_the_books_pauses_sync_and_changes_nothing() {
    use tally_store::sync::{ChangeOp, RowChange};
    let mut w = World::new(2);
    let key = w.key.clone();
    // A hostile or buggy device pushes an unbalanced entry, properly encrypted.
    let bank = code(&w.devs[0].store, "1000");
    let changes = vec![
        RowChange {
            table: "journal_entries".into(),
            pk: vec![serde_json::json!(5i64 << 40)],
            op: ChangeOp::Insert,
            row: Some(serde_json::json!({"id": 5i64 << 40, "date": "2026-01-01", "memo": "bad", "source": "manual", "reverses_id": null, "voided_by_id": null, "created_at": "2026-01-01 00:00:00", "payee_id": null, "person_id": null, "goal_id": null}).as_object().unwrap().clone()),
            hlc: "0000000000000001-00000000-0005".into(),
        },
        RowChange {
            table: "journal_lines".into(),
            pk: vec![serde_json::json!(5i64 << 40)],
            op: ChangeOp::Insert,
            row: Some(serde_json::json!({"id": 5i64 << 40, "entry_id": 5i64 << 40, "account_id": bank, "amount": 100, "memo": null, "cleared": "uncleared", "reconciliation_id": null}).as_object().unwrap().clone()),
            hlc: "0000000000000001-00000001-0005".into(),
        },
    ];
    let body = serde_json::json!({"version": 1, "device": 5, "changes": changes});
    let sealed = key.seal_batch("5-1-2", serde_json::to_vec(&body).unwrap().as_slice()).unwrap();
    let mut evil = w.server.connect();
    evil.push_batch(&key.book_id, 5, "5-1-2", sealed).unwrap();
    let before = w.devs[0].store.sync_dump().unwrap();
    let err = w.devs[0].sync(&key).unwrap_err();
    assert!(err.to_string().contains("out of balance"), "{err}");
    assert_eq!(w.devs[0].store.sync_dump().unwrap(), before);
    assert_eq!(w.devs[0].store.sync_last_seq().unwrap(), 0, "it did not move past the bad batch");
    w.finish();
}

// ---------------------------------------------------------------- tampering

#[test]
fn a_tampered_wrong_book_or_wrong_key_batch_is_never_applied() {
    let mut w = World::new(2);
    let (bank, sales) = (code(&w.devs[0].store, "1000"), code(&w.devs[0].store, "4000"));
    post(&mut w.devs[0].store, d(2026, 1, 1), "real", bank, sales, 100);
    w.devs[0].sync(&w.key).unwrap();
    let book = w.key.book_id.clone();
    let good = w.server.batches(&book)[0].clone();
    let before = w.devs[1].store.sync_dump().unwrap();

    // 1. A flipped byte.
    let mut bad = good.data.clone();
    let last = bad.len() - 1;
    bad[last] ^= 1;
    w.server.replace_batch_data(&book, good.seq, bad);
    assert!(matches!(w.devs[1].sync(&w.key), Err(SyncError::Authentication)));
    assert_eq!(w.devs[1].store.sync_dump().unwrap(), before);
    assert_eq!(w.devs[1].store.sync_last_seq().unwrap(), 0);

    // 2. A batch sealed for another book.
    let other_book = SyncKey::new("another-book", *w.key.key_bytes());
    let body = key_bytes_of_batch(&w, &good);
    w.server.replace_batch_data(&book, good.seq, other_book.seal_batch(&good.batch_id, &body).unwrap());
    assert!(matches!(w.devs[1].sync(&w.key), Err(SyncError::Authentication)));

    // 3. A batch sealed with another key.
    let stranger = SyncKey::new(book.clone(), [42; 32]);
    w.server.replace_batch_data(&book, good.seq, stranger.seal_batch(&good.batch_id, &body).unwrap());
    assert!(matches!(w.devs[1].sync(&w.key), Err(SyncError::Authentication)));

    // 4. The real one, restored, applies.
    w.server.replace_batch_data(&book, good.seq, good.data.clone());
    w.devs[1].sync(&w.key).unwrap();
    assert_eq!(rows(&w.devs[1].store, "journal_entries").len(), 1);

    // A device with the wrong key can't read anything.
    let mut outsider = w.server.connect();
    let dir = temp_dir();
    let wrong = SyncKey::new(book.clone(), [1; 32]);
    assert!(matches!(join_from_snapshot(&mut outsider, &wrong, &dir.join("x.tallybook")), Err(SyncError::Authentication)));
    assert!(!dir.join("x.tallybook").exists());
    let _ = std::fs::remove_dir_all(dir);
    w.finish();
}

/// The plaintext of a batch, for the test that re-seals it.
fn key_bytes_of_batch(w: &World, b: &RemoteBatch) -> Vec<u8> {
    w.key.open_batch(&b.batch_id, &b.data).unwrap()
}

// ---------------------------------------------------------------- snapshots

#[test]
fn a_new_device_from_a_snapshot_plus_later_batches_equals_the_source() {
    let mut w = World::new(2);
    let (bank, sales) = (code(&w.devs[0].store, "1000"), code(&w.devs[0].store, "4000"));
    for i in 0..5 {
        post(&mut w.devs[0].store, d(2026, 1, 1 + i), &format!("before {i}"), bank, sales, 100 + i as i64);
    }
    w.sync_everyone();
    w.devs[0].snapshot(&w.key).unwrap();
    // Things keep happening after the snapshot.
    post(&mut w.devs[1].store, d(2026, 2, 1), "after", bank, sales, 999);
    w.devs[0].store.rename_account(bank, "Main checking").unwrap();
    w.sync_everyone();
    let mut net = w.server.connect();
    let mut late = join_from_snapshot(&mut net, &w.key, &w.dir.join("late.tallybook")).unwrap();
    assert_eq!(late.sync_device().unwrap(), 2, "the server numbered it");
    assert_eq!(late.sync_outbox_len().unwrap(), 0);
    sync_once(&mut late, &mut net, &w.key).unwrap();
    assert_same_books(&[&w.devs[0].store, &late]);
    // It can make changes of its own, in its own id range.
    let mine = late.create_account(&NewAccount::new("From the phone", AccountSubtype::Expense)).unwrap();
    assert!(mine >= 2 << 40);
    sync_once(&mut late, &mut net, &w.key).unwrap();
    w.sync_everyone();
    assert_eq!(w.devs[0].store.account(mine).unwrap().name, "From the phone");
    w.finish();
}

#[test]
fn a_snapshot_lets_the_server_forget_old_batches() {
    let mut w = World::new(2);
    let (bank, sales) = (code(&w.devs[0].store, "1000"), code(&w.devs[0].store, "4000"));
    for i in 0..6 {
        post(&mut w.devs[(i % 2) as usize].store, d(2026, 1, 1 + i), "x", bank, sales, 10 + i as i64);
        w.sync_everyone();
    }
    w.devs[0].snapshot(&w.key).unwrap();
    w.sync_everyone();
    let dropped = w.server.prune(&w.key.book_id, &[0, 1]);
    assert!(dropped >= 5, "dropped {dropped}");
    let mut net = w.server.connect();
    let mut late = join_from_snapshot(&mut net, &w.key, &w.dir.join("late.tallybook")).unwrap();
    sync_once(&mut late, &mut net, &w.key).unwrap();
    assert_same_books(&[&w.devs[0].store, &w.devs[1].store, &late]);
    w.finish();
}

#[test]
fn a_tampered_snapshot_is_refused_and_nothing_is_left_behind() {
    let w = World::new(1);
    let book = w.key.book_id.clone();
    let (_, mut data) = w.server.connect().get_snapshot(&book).unwrap().unwrap();
    let mid = data.len() / 2;
    data[mid] ^= 4;
    w.server.replace_snapshot_data(&book, data);
    let mut net = w.server.connect();
    let dest = w.dir.join("joined.tallybook");
    assert!(matches!(join_from_snapshot(&mut net, &w.key, &dest), Err(SyncError::Authentication)));
    assert!(!dest.exists());
    // And with no snapshot at all.
    let empty = MemoryServer::new();
    assert!(matches!(join_from_snapshot(&mut empty.connect(), &w.key, &dest), Err(SyncError::NoSnapshot)));
    w.finish();
}

#[test]
fn syncing_needs_sync_turned_on() {
    let mut store = Store::open_in_memory().unwrap();
    let server = MemoryServer::new();
    let key = SyncKey::generate().unwrap();
    assert!(matches!(sync_once(&mut store, &mut server.connect(), &key), Err(SyncError::NotEnabled)));
    assert!(matches!(create_snapshot(&store, &mut server.connect(), &key), Err(SyncError::NotEnabled)));
}

#[test]
fn an_existing_book_joins_sync_with_its_history_in_a_snapshot_not_the_outbox() {
    for fixture in ["business-v0.2.0", "household-v0.2.0"] {
        let from = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../tally-api/tests/fixtures/{fixture}.tallybook"));
        let dir = temp_dir();
        std::fs::copy(from, dir.join("book.tallybook")).unwrap();
        let mut original = Store::open(dir.join("book.tallybook")).unwrap();
        assert_eq!(original.sync_outbox_len().unwrap(), 0, "no backfill");
        original.sync_enable().unwrap();
        assert_eq!(original.sync_outbox_len().unwrap(), 0, "turning sync on doesn't copy old rows into the outbox either");
        let server = MemoryServer::new();
        let key = SyncKey::generate().unwrap();
        let mut net = server.connect();
        create_snapshot(&original, &mut net, &key).unwrap();
        let mut phone_net = server.connect();
        let mut phone = join_from_snapshot(&mut phone_net, &key, &dir.join("phone.tallybook")).unwrap();
        assert_same_books(&[&original, &phone]);
        assert!(!rows(&phone, "journal_entries").is_empty(), "the old history came with the snapshot");
        // And both can carry on.
        let accounts = original.accounts().unwrap();
        let (a, b) = (accounts[0].clone(), accounts.iter().find(|x| x.scope == accounts[0].scope && x.id != accounts[0].id).unwrap().clone());
        original.post_entry(&tally_core::NewEntry::new(d(2030, 1, 1), "new").line(tally_core::EntryLine::debit(a.id, Cents(5))).line(tally_core::EntryLine::credit(b.id, Cents(5)))).unwrap();
        sync_once(&mut original, &mut net, &key).unwrap();
        sync_once(&mut phone, &mut phone_net, &key).unwrap();
        assert_same_books(&[&original, &phone]);
        drop((original, phone));
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn an_encrypted_book_syncs_too() {
    let dir = temp_dir();
    let book_key = tally_store::crypto::generate_key().unwrap();
    let mut store = Store::open_encrypted(dir.join("secret.tallybook"), &book_key).unwrap();
    store.seed_default_chart().unwrap();
    store.sync_enable().unwrap();
    let server = MemoryServer::new();
    let key = SyncKey::generate().unwrap();
    let mut net = server.connect();
    create_snapshot(&store, &mut net, &key).unwrap();
    let joined = join_from_snapshot(&mut server.connect(), &key, &dir.join("joined.tallybook")).unwrap();
    assert_same_books(&[&store, &joined]);
    let _ = std::fs::remove_dir_all(dir);
}
