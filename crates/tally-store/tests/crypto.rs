// Amounts are written as dollars_cents (e.g. 1200_00), hence the grouping.
#![allow(clippy::inconsistent_digit_grouping)]

use chrono::NaiveDate;
use std::path::PathBuf;
use tally_core::{AccountSubtype, Cents, EntryLine, NewEntry};
use tally_store::crypto::{generate_key, is_encrypted_file, parse_recovery_key, recovery_key};
use tally_store::*;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tally-crypto-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A small book with one memorable entry.
fn plain_book(path: &std::path::Path) -> Store {
    let mut s = Store::open(path).unwrap();
    s.seed_default_chart().unwrap();
    s.set_pref("company.name", Some("Secret Sauce LLC")).unwrap();
    let bank = s.create_account(&NewAccount::new("Checking", AccountSubtype::Bank).code("1000")).unwrap();
    let sales = s.account_by_code("4000").unwrap().unwrap().id;
    s.post_entry(&NewEntry::new(NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(), "Payment from Very Confidential Client").line(EntryLine::debit(bank, Cents(12_345_67))).line(EntryLine::credit(sales, Cents(12_345_67)))).unwrap();
    s
}

fn contains(path: &std::path::Path, needle: &str) -> bool {
    let bytes = std::fs::read(path).unwrap();
    bytes.windows(needle.len()).any(|w| w == needle.as_bytes())
}

#[test]
fn encrypted_export_hides_everything_and_needs_the_key() {
    let d = dir("export");
    let plain = d.join("plain.tallybook");
    let enc = d.join("enc.tallybook");
    let store = plain_book(&plain);
    let key = generate_key().unwrap();
    store.export_encrypted(&enc, &key).unwrap();
    drop(store);

    assert!(!is_encrypted_file(&plain).unwrap());
    assert!(is_encrypted_file(&enc).unwrap());
    // Sanity: the plain file really does contain the secret text…
    assert!(contains(&plain, "Very Confidential Client"));
    // …and the encrypted one contains none of it, not even SQLite's header.
    assert!(!contains(&enc, "Very Confidential Client"));
    assert!(!contains(&enc, "Secret Sauce"));
    assert!(!contains(&enc, "SQLite format 3"));

    // Without a key it reports Locked; with the wrong key it's refused.
    assert!(matches!(Store::open(&enc), Err(StoreError::Locked)));
    assert!(Store::open_encrypted(&enc, &generate_key().unwrap()).is_err());

    // The right key (here via the recovery key) opens it, schema version intact.
    let k = parse_recovery_key(&recovery_key(&key)).unwrap();
    let s = Store::open_encrypted(&enc, &k).unwrap();
    assert!(s.is_encrypted());
    assert_eq!(s.pref("company.name").unwrap().as_deref(), Some("Secret Sauce LLC"));
    let checking = s.account_by_code("1000").unwrap().unwrap().id;
    assert_eq!(s.account_balance(checking, NaiveDate::from_ymd_opt(2026, 12, 31).unwrap()).unwrap(), Cents(12_345_67));
    assert!(s.balance_sheet(NaiveDate::from_ymd_opt(2026, 12, 31).unwrap()).unwrap().is_balanced());
}

#[test]
fn backups_of_encrypted_books_stay_encrypted_and_decrypt_round_trips() {
    let d = dir("backup");
    let enc = d.join("enc.tallybook");
    let key = generate_key().unwrap();
    plain_book(&d.join("plain.tallybook")).export_encrypted(&enc, &key).unwrap();

    let s = Store::open_encrypted(&enc, &key).unwrap();
    let backup = d.join("backup.tallybook");
    s.backup_to(&backup).unwrap();
    assert!(is_encrypted_file(&backup).unwrap(), "a backup must never be a plaintext copy");
    assert!(!contains(&backup, "Very Confidential Client"));
    assert!(Store::open_encrypted(&backup, &key).is_ok());

    // Turning encryption off produces an ordinary book again.
    let back = d.join("decrypted.tallybook");
    s.export_plain(&back).unwrap();
    assert!(!is_encrypted_file(&back).unwrap());
    let p = Store::open(&back).unwrap();
    assert_eq!(p.pref("company.name").unwrap().as_deref(), Some("Secret Sauce LLC"));
    assert!(s.export_plain(&back).is_err(), "never overwrites");
}
