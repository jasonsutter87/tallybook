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

/// Reads a header pragma straight from a file, unlocking it first if a key is given.
fn header_value(path: &std::path::Path, key: Option<&crypto::BookKey>, pragma: &str) -> i64 {
    let conn = rusqlite::Connection::open(path).unwrap();
    if let Some(key) = key {
        conn.pragma_update(None, "key", format!("x'{}'", key.iter().map(|b| format!("{b:02x}")).collect::<String>())).unwrap();
    }
    conn.pragma_query_value(None, pragma, |r| r.get(0)).unwrap()
}

#[test]
fn every_copy_of_a_book_says_what_it_is() {
    let d = dir("identity");
    let plain = d.join("plain.tallybook");
    let store = plain_book(&plain);
    let key = generate_key().unwrap();
    let enc = d.join("enc.tallybook");
    store.export_encrypted(&enc, &key).unwrap();
    let backup = d.join("backup.tallybook");
    store.backup_to(&backup).unwrap();
    drop(store);
    let enc_store = Store::open_encrypted(&enc, &key).unwrap();
    let decrypted = d.join("decrypted.tallybook");
    enc_store.export_plain(&decrypted).unwrap();
    drop(enc_store);

    for (path, key) in [(&plain, None), (&backup, None), (&decrypted, None), (&enc, Some(&key))] {
        assert_eq!(header_value(path, key, "application_id"), i64::from(APPLICATION_ID), "{}", path.display());
        assert_eq!(header_value(path, key, "user_version"), 12, "{}", path.display());
    }
    // "TALY", so a hex dump of the header reads as the app's name.
    assert_eq!(APPLICATION_ID.to_be_bytes(), *b"TALY");
}

#[test]
fn another_programs_database_is_refused_and_left_alone() {
    let d = dir("foreign");
    let path = d.join("photos.sqlite");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("PRAGMA application_id = 1234; CREATE TABLE photos (id INTEGER PRIMARY KEY);").unwrap();
    drop(conn);
    assert!(matches!(Store::open(&path), Err(StoreError::Rule(m)) if m.contains("not a Tallybook book")));
    // Nothing of ours was written into it.
    assert_eq!(header_value(&path, None, "user_version"), 0);
}

/// SQLCipher writes none of its settings into the file, so a book opens only
/// with the exact settings it was written with. Tallybook uses SQLCipher 4's
/// defaults, and docs/FILE_FORMAT.md promises any SQLCipher 4 tool can open a
/// book with `PRAGMA cipher_compatibility = 4`. If a library upgrade ever
/// changes a default, this fails before any book is written the new way.
#[test]
fn encrypted_books_use_plain_sqlcipher_4_settings() {
    let d = dir("cipher");
    let key = generate_key().unwrap();
    let enc = d.join("enc.tallybook");
    plain_book(&d.join("plain.tallybook")).export_encrypted(&enc, &key).unwrap();
    let raw_key = format!("x'{}'", key.iter().map(|b| format!("{b:02x}")).collect::<String>());

    let conn = rusqlite::Connection::open(&enc).unwrap();
    conn.pragma_update(None, "key", &raw_key).unwrap();
    let setting = |p: &str| -> String { conn.query_row(&format!("PRAGMA {p}"), [], |r| r.get(0)).unwrap() };
    assert!(setting("cipher_version").starts_with("4."), "SQLCipher major version changed: {}", setting("cipher_version"));
    assert_eq!(setting("cipher_page_size"), "4096");
    assert_eq!(setting("kdf_iter"), "256000");
    assert_eq!(setting("cipher_hmac_algorithm"), "HMAC_SHA512");
    assert_eq!(setting("cipher_kdf_algorithm"), "PBKDF2_HMAC_SHA512");
    assert_eq!(setting("cipher_plaintext_header_size"), "0");
    drop(conn);

    // What a stranger with only the file and its key would type.
    let conn = rusqlite::Connection::open(&enc).unwrap();
    conn.execute_batch("PRAGMA cipher_compatibility = 4;").unwrap();
    conn.pragma_update(None, "key", &raw_key).unwrap();
    let name: String = conn.query_row("SELECT value FROM settings WHERE key = 'pref.company.name'", [], |r| r.get(0)).unwrap();
    assert_eq!(name, "Secret Sauce LLC");
}

#[test]
fn fingerprints_cover_every_table_and_survive_encryption() {
    let d = dir("fingerprint");
    let store = plain_book(&d.join("book.tallybook"));
    let before = store.fingerprint().unwrap();

    store.export_plain(&d.join("copy.tallybook")).unwrap();
    let copy = Store::open(d.join("copy.tallybook")).unwrap();
    assert_eq!(copy.fingerprint().unwrap(), before);
    let key = generate_key().unwrap();
    store.export_encrypted(&d.join("enc.tallybook"), &key).unwrap();
    assert_eq!(Store::open_encrypted(d.join("enc.tallybook"), &key).unwrap().fingerprint().unwrap(), before);

    // A change in a table other than accounts and the journal still shows.
    copy.set_pref("company.email", Some("a@b.co")).unwrap();
    assert_ne!(copy.fingerprint().unwrap(), before);
    let other = Store::open(d.join("copy.tallybook")).unwrap();
    other.debug_execute("DELETE FROM settings WHERE key = 'pref.company.email'").unwrap();
    assert_eq!(other.fingerprint().unwrap(), before);
}
