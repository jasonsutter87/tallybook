//! Tests of `App::call` as the UI uses it: method names and JSON in, JSON
//! out, against real book files in fresh temp folders. Tests that need the
//! crate's private pieces (licenses, recent books) live here; the rest are in
//! `crates/tally-api/tests/`.

use chrono::Datelike;
use tally_core::AccountSubtype;
use super::*;
use serde_json::json;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tallybook-test-{name}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// An app licensed for everything, as most tests want.
fn app(name: &str) -> (App, PathBuf) {
    let (app, dir) = free_app(name);
    let license = crate::license::License {
        tier: crate::license::Tier::Tax,
        key: "TEST-KEY-0000".into(),
        instance_id: "test".into(),
        email: None,
        activated_on: today(),
        tax_years: vec![],
        earlier: vec![],
    };
    crate::license::save(&dir.join("config"), &license).unwrap();
    (app, dir)
}

fn free_app(name: &str) -> (App, PathBuf) {
    let dir = temp_dir(name);
    (App::new(Arc::new(MemorySecrets::default()), dir.join("config")).with_file_remover(|p| std::fs::remove_file(p)), dir)
}

#[tokio::test]
async fn free_copy_keeps_households_and_never_locks_a_book() {
    let (app, dir) = free_app("free");
    let st = app.call("license.status", Value::Null).await.unwrap();
    assert_eq!(st["tier"], "free");
    assert_eq!(app.call("book.create", json!({ "path": dir.join("b.tallybook"), "company_name": "Biz" })).await.unwrap_err().code, "license");
    app.call("book.create", json!({ "path": dir.join("h.tallybook"), "company_name": "Home", "sides": ["household"] })).await.unwrap();
    assert_eq!(app.call("book.add_side", json!({ "scope": "business" })).await.unwrap_err().code, "license");

    // The demo, and any business book made before, still open and work; only tax prep is held back.
    app.call("book.create_demo", json!({ "path": dir.join("demo.tallybook") })).await.unwrap();
    app.call("dashboard", json!({})).await.unwrap();
    let year = today().year();
    assert_eq!(app.call("tax.schedule_c", json!({ "year": year })).await.unwrap_err().code, "license");
    assert!(app.call("tax.years", Value::Null).await.is_ok(), "the Taxes page can still say what it offers");

    // A Business license unlocks the side but not tax prep.
    let business = crate::license::License {
        tier: crate::license::Tier::Business,
        key: "K".into(),
        instance_id: "i".into(),
        email: None,
        activated_on: today(),
        tax_years: vec![],
        earlier: vec![],
    };
    crate::license::save(&dir.join("config"), &business).unwrap();
    app.call("book.create", json!({ "path": dir.join("b.tallybook"), "company_name": "Biz" })).await.unwrap();
    assert_eq!(app.call("tax.export", json!({ "year": year })).await.unwrap_err().code, "license");

    // Business + Tax covers the season it was bought in, not every year after.
    let tax = crate::license::License {
        tier: crate::license::Tier::Tax,
        key: "T".into(),
        instance_id: "i".into(),
        email: None,
        activated_on: today(),
        tax_years: vec![year - 1, year],
        earlier: vec![],
    };
    crate::license::save(&dir.join("config"), &tax).unwrap();
    app.call("tax.schedule_c", json!({ "year": year })).await.unwrap();
    let next = app.call("tax.schedule_c", json!({ "year": year + 1 })).await.unwrap_err();
    assert!(next.code == "license" || next.code == "unsupported_year", "{next:?}");
    let years = app.call("tax.years", Value::Null).await.unwrap();
    assert_eq!(years["licensed"], json!([year - 1, year]));
}

#[tokio::test]
async fn book_lifecycle_and_recent_list() {
    let (app, dir) = app("book");
    let path = dir.join("Acme.tallybook");
    assert_eq!(app.call("dashboard", Value::Null).await.unwrap_err().code, "no_book");

    app.call("book.create", json!({ "path": path, "company_name": "Acme LLC" })).await.unwrap();
    let status = app.call("book.status", Value::Null).await.unwrap();
    assert_eq!(status["company_name"], "Acme LLC");
    assert_eq!(app.call("book.recent", Value::Null).await.unwrap().as_array().unwrap().len(), 1);

    let backup = app.call("book.backup", Value::Null).await.unwrap();
    assert!(PathBuf::from(backup.as_str().unwrap()).exists());

    app.call("book.close", Value::Null).await.unwrap();
    app.call("book.open", json!({ "path": path })).await.unwrap();
    assert!(app.is_open());
}

#[tokio::test]
async fn end_to_end_through_the_dispatcher() {
    let (app, dir) = app("e2e");
    app.call("book.create", json!({ "path": dir.join("b.tallybook"), "company_name": "Test Co" })).await.unwrap();

    let checking = app
        .call("accounts.create", json!({ "name": "Checking", "subtype": "bank", "opening_balance": 100000, "opening_date": "2026-01-01" }))
        .await
        .unwrap();
    let accounts = app.call("accounts.list", Value::Null).await.unwrap();
    let mine = accounts.as_array().unwrap().iter().find(|a| a["id"] == checking).unwrap();
    assert_eq!(mine["balance"], 100000);
    assert_eq!(mine["reconcilable"], true);

    let csv = "Date,Description,Amount\n2026-02-01,FIGMA,-45.00\n2026-02-02,CLIENT,500.00\n";
    let preview = app.call("import.csv_preview", json!({ "content": csv })).await.unwrap();
    let summary = app
        .call("import.csv", json!({ "content": csv, "mapping": preview["mapping"], "account_id": checking }))
        .await
        .unwrap();
    assert_eq!(summary["added"], 2);

    let inbox = app.call("inbox.list", Value::Null).await.unwrap();
    let figma = inbox.as_array().unwrap().iter().find(|i| i["description"] == "FIGMA").unwrap();
    let software = app.call("accounts.list", Value::Null).await.unwrap().as_array().unwrap().iter().find(|a| a["code"] == "6500").unwrap()["id"].clone();
    app.call("inbox.accept", json!({ "id": figma["id"], "account_id": software, "remember": true })).await.unwrap();
    assert_eq!(app.call("rules.list", Value::Null).await.unwrap().as_array().unwrap().len(), 1);

    let dash = app.call("dashboard", Value::Null).await.unwrap();
    assert_eq!(dash["inbox_count"], 1);
    assert_eq!(dash["accounts"].as_array().unwrap().len(), 1);

    app.call("settings.set", json!({ "prefs": { "company.email": "hi@test.co" }, "lock_date": "2025-12-31" })).await.unwrap();
    let settings = app.call("settings.get", Value::Null).await.unwrap();
    assert_eq!(settings["prefs"]["company.email"], "hi@test.co");
    assert_eq!(settings["lock_date"], "2025-12-31");
    app.call("settings.set", json!({ "lock_date": null })).await.unwrap();
    assert!(app.call("settings.get", Value::Null).await.unwrap()["lock_date"].is_null());

    assert_eq!(app.call("nope", Value::Null).await.unwrap_err().code, "unknown_method");
    assert_eq!(app.call("entries.get", json!({ "id": "x" })).await.unwrap_err().code, "bad_params");
    assert_eq!(app.call("entries.get", json!({ "id": 999 })).await.unwrap_err().code, "not_found");
}

#[tokio::test]
async fn plaid_requires_configuration_and_stores_keys_in_secrets() {
    let (app, dir) = app("plaid");
    app.call("book.create", json!({ "path": dir.join("b.tallybook"), "company_name": "X" })).await.unwrap();
    assert_eq!(app.call("bank.plaid_start", Value::Null).await.unwrap_err().code, "plaid_not_configured");
    app.call("bank.plaid_config_set", json!({ "client_id": "abc123456", "secret": "s", "environment": "sandbox" })).await.unwrap();
    let cfg = app.call("bank.plaid_config_get", Value::Null).await.unwrap();
    assert_eq!(cfg["configured"], true);
    assert_eq!(cfg["client_id_hint"], "…3456");
}

#[tokio::test]
async fn demo_book_is_consistent() {
    let (app, dir) = app("demo");
    app.call("book.create_demo", json!({ "path": dir.join("demo.tallybook") })).await.unwrap();
    let tb = app.call("reports.trial_balance", Value::Null).await.unwrap();
    assert_eq!(tb["total_debits"], tb["total_credits"]);
    let bs = app.call("reports.balance_sheet", Value::Null).await.unwrap();
    assert_eq!(bs["total_assets"].as_i64().unwrap(), bs["total_liabilities"].as_i64().unwrap() + bs["total_equity"].as_i64().unwrap());
    let dash = app.call("dashboard", Value::Null).await.unwrap();
    assert!(dash["inbox_count"].as_i64().unwrap() > 0, "something to review");
    assert!(dash["receivable"].as_i64().unwrap() > 0, "open invoices");
    assert!(dash["cash"].as_i64().unwrap() > 0, "{dash}");
    // The card balance in the books should agree with what the card's feed implies.
    let inbox = app.call("inbox.list", Value::Null).await.unwrap();
    let unreviewed_dupes = inbox.as_array().unwrap().iter().filter(|i| i["suggestion"].is_null()).count();
    eprintln!("inbox {} items ({} without suggestion), cash {}, AR {}", inbox.as_array().unwrap().len(), unreviewed_dupes, dash["cash"], dash["receivable"]);
}

#[tokio::test]
async fn tax_center_end_to_end() {
    let (app, dir) = app("tax");
    app.call("book.create_demo", json!({ "path": dir.join("demo.tallybook") })).await.unwrap();
    let year = today().year();

    let sc = app.call("tax.schedule_c", json!({ "year": year })).await.unwrap();
    let report = &sc["report"];
    assert!(report["schedule"]["line1_gross_receipts"].as_i64().unwrap() > 0);
    assert!(report["unconfirmed"].as_i64().unwrap() > 0, "suggestions start unconfirmed");
    app.call("tax.confirm_all", json!({ "year": year })).await.unwrap();
    let sc = app.call("tax.schedule_c", json!({ "year": year })).await.unwrap();
    assert_eq!(sc["report"]["unconfirmed"], 0);

    let est = app.call("tax.estimate", json!({ "year": year })).await.unwrap();
    assert_eq!(est["plan_method"], "safe_harbor");
    assert_eq!(est["safe_harbor"], 2_460_000, "AGI under $150k: 100% of last year");
    assert_eq!(est["quarters"].as_array().unwrap().len(), 4);
    assert!(est["estimate"]["se"]["total"].as_i64().unwrap() > 0);

    let r1099 = app.call("tax.1099", json!({ "year": year })).await.unwrap();
    let maya = r1099["rows"].as_array().unwrap().iter().find(|r| r["name"] == "Maya Chen Illustration").expect("Maya listed");
    assert!(maya["paid_reportable"].as_i64().unwrap() > 0, "{maya}");

    let out = app.call("tax.export", json!({ "year": year })).await.unwrap();
    let out = PathBuf::from(out.as_str().unwrap());
    for f in ["README.txt", "Schedule C worksheet.csv", "Schedule C mapping.csv", "General ledger.csv", "1099 summary.csv", "Estimated tax payments.csv", "Tax summary.pdf", "General ledger.pdf"] {
        assert!(out.join(f).exists(), "{f}");
    }
    assert!(std::fs::read_to_string(out.join("README.txt")).unwrap().contains("not tax advice"));
    let pdf = std::fs::read(out.join("Tax summary.pdf")).unwrap();
    assert!(pdf.starts_with(b"%PDF-") && pdf.ends_with(b"%%EOF\n"));

    assert_eq!(app.call("tax.estimate", json!({ "year": 2019 })).await.unwrap_err().code, "unsupported_year");
}

#[tokio::test]
async fn tax_export_csv_shows_formula_like_memos_as_text() {
    let (app, dir) = app("tax-csv");
    app.call("book.create", json!({ "path": dir.join("Shop.tallybook"), "company_name": "Shop" })).await.unwrap();
    let accounts = app.call("accounts.list", Value::Null).await.unwrap();
    let id = |code: &str| accounts.as_array().unwrap().iter().find(|a| a["code"] == code).unwrap()["id"].clone();
    let date = NaiveDate::from_ymd_opt(today().year(), 1, 2).unwrap();
    app.call("entries.post", json!({ "date": date, "memo": "=HYPERLINK(\"https://example.com\")", "lines": [
        { "account_id": id("6500"), "amount": 1250, "memo": "@SUM(A1)" },
        { "account_id": id("4000"), "amount": -1250 }
    ] }))
    .await
    .unwrap();
    let out = PathBuf::from(app.call("tax.export", json!({ "year": today().year() })).await.unwrap().as_str().unwrap().to_string());
    let csv = std::fs::read_to_string(out.join("General ledger.csv")).unwrap();
    assert!(csv.contains("\"'=HYPERLINK("), "{csv}");
    assert!(csv.contains(",'@SUM(A1)"), "{csv}");
    assert!(!csv.contains(",=") && !csv.contains(",@"), "{csv}");
    assert!(csv.contains(",12.50,"), "amounts stay plain numbers: {csv}");
}

/// The tax package is plain text: for an encrypted book it goes only where
/// the user chose, never silently beside the book.
#[tokio::test]
async fn an_encrypted_books_tax_package_needs_a_chosen_folder() {
    let (app, dir) = app("tax-encrypted");
    let book = dir.join("books");
    app.call("book.create_demo", json!({ "path": book.join("demo.tallybook") })).await.unwrap();
    app.call("book.encrypt", Value::Null).await.unwrap();
    let year = today().year();
    let err = app.call("tax.export", json!({ "year": year })).await.unwrap_err();
    assert_eq!(err.code, "rule");
    assert!(!book.join(format!("Tax {year}")).exists(), "nothing written beside the book");

    let chosen = dir.join("For the accountant");
    assert_eq!(app.call("tax.export", json!({ "year": year, "dir": chosen })).await.unwrap_err().code, "rule", "the folder must exist");
    std::fs::create_dir_all(&chosen).unwrap();
    let out = app.call("tax.export", json!({ "year": year, "dir": chosen })).await.unwrap();
    assert_eq!(PathBuf::from(out.as_str().unwrap()), chosen.join(format!("Tax {year}")));
    assert!(chosen.join(format!("Tax {year}")).join("General ledger.csv").exists());
}

#[tokio::test]
async fn delete_book_tears_everything_down() {
    let (app, dir) = app("delete");
    let path = dir.join("Gone.tallybook");
    app.call("book.create", json!({ "path": path, "company_name": "Gone Co" })).await.unwrap();
    app.call("book.backup", Value::Null).await.unwrap();
    // A bank connection whose credential lives in the keychain.
    app.secrets.set("simplefin:00000000-0000-4000-8000-000000000001", "https://user:pass@bridge.example/simplefin").unwrap();
    app.run(|s| s.create_connection(Provider::Simplefin, "CU", None, Some("simplefin:00000000-0000-4000-8000-000000000001"))).unwrap();

    let wrong = app.call("book.delete", json!({ "path": path, "confirm_name": "gone co" })).await.unwrap_err();
    assert_eq!(wrong.code, "rule", "the name must match exactly");
    assert!(path.exists());
    assert!(app.is_open(), "a wrong confirmation must not close the open book");

    let r = app.call("book.delete", json!({ "path": path, "confirm_name": "Gone Co", "include_backups": true })).await.unwrap();
    assert!(!path.exists());
    assert_eq!(r["keychain_entries_removed"], 1);
    assert!(app.secrets.get("simplefin:00000000-0000-4000-8000-000000000001").unwrap().is_none(), "credential removed from the keychain");
    assert!(std::fs::read_dir(dir.join("Backups")).unwrap().next().is_none(), "backups removed too");
    assert!(!app.is_open(), "the open book was closed first");
    assert!(app.call("book.recent", Value::Null).await.unwrap().as_array().unwrap().is_empty());

    let not_a_book = dir.join("notes.txt");
    std::fs::write(&not_a_book, "hi").unwrap();
    assert_eq!(app.call("book.delete", json!({ "path": not_a_book, "confirm_name": "x" })).await.unwrap_err().code, "rule");
}

/// A connection's keychain name comes from the book file. Names Tallybook
/// didn't make (`<provider>:<uuid>`) are never read or deleted.
#[tokio::test]
async fn a_book_cant_name_keychain_entries_it_doesnt_own() {
    let (app, dir) = app("secret-ref");
    let path = dir.join("Shared.tallybook");
    app.call("book.create", json!({ "path": path, "company_name": "Shared" })).await.unwrap();
    app.secrets.set("plaid.credentials", "{\"client_id\":\"mine\"}").unwrap();
    app.secrets.set("book-key:1234", "abcd").unwrap();
    let plaid_ok = "plaid:00000000-0000-4000-8000-0000000000aa";
    app.secrets.set(plaid_ok, "simplefin-looking").unwrap();
    let (a, b, c) = app
        .run(|s| {
            Ok((
                s.create_connection(Provider::Plaid, "Sneaky", None, Some("plaid.credentials"))?,
                s.create_connection(Provider::Simplefin, "Sneaky 2", None, Some("book-key:1234"))?,
                // A real-looking name, but for the other provider.
                s.create_connection(Provider::Simplefin, "Sneaky 3", None, Some(plaid_ok))?,
            ))
        })
        .unwrap();
    for id in [a, b, c] {
        let err = app.call("bank.feed_accounts", json!({ "connection_id": id })).await.unwrap_err();
        assert!(err.message.contains("no saved credentials"), "{err:?}");
    }
    app.call("bank.disconnect", json!({ "connection_id": a })).await.unwrap();
    assert!(app.secrets.get("plaid.credentials").unwrap().is_some(), "disconnecting left the Plaid keys alone");

    let r = app.call("book.delete", json!({ "path": path, "confirm_name": "Shared" })).await.unwrap();
    assert_eq!(r["keychain_entries_removed"], 0, "{r}");
    for key in ["plaid.credentials", "book-key:1234", plaid_ok] {
        assert!(app.secrets.get(key).unwrap().is_some(), "{key} survived deleting the book");
    }
}

/// recent.json, keys.json and license.json are written whole, readable only
/// by the user, and license.json holds no whole license key.
#[tokio::test]
async fn config_files_are_private_and_license_keys_live_in_the_keychain() {
    let (app, dir) = free_app("config-files");
    app.call("book.create", json!({ "path": dir.join("Home.tallybook"), "company_name": "Home", "sides": ["household"] })).await.unwrap();
    app.call("book.encrypt", Value::Null).await.unwrap();
    let license = crate::license::License {
        tier: crate::license::Tier::Business,
        key: "ABCD-1234-SECRET-WXYZ".into(),
        instance_id: "inst-1".into(),
        email: None,
        activated_on: today(),
        tax_years: vec![],
        earlier: vec![crate::license::Activation { key: "OLD-KEY-HIDDEN-9999".into(), instance_id: "inst-0".into() }],
    };
    app.save_license(&license).unwrap();

    let config = dir.join("config");
    let text = std::fs::read_to_string(config.join("license.json")).unwrap();
    assert!(!text.contains("SECRET") && !text.contains("HIDDEN"), "{text}");
    assert_eq!(app.load_license().unwrap(), license, "the keys come back from the keychain");
    assert_eq!(app.call("license.status", Value::Null).await.unwrap()["key_hint"], "WXYZ");
    assert_eq!(app.call("license.status", Value::Null).await.unwrap()["tier"], "business");

    let names: Vec<String> = std::fs::read_dir(&config).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    assert!(names.iter().all(|n| !n.ends_with(".tmp")), "no temporary files left: {names:?}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for f in ["recent.json", "keys.json", "license.json"] {
            let mode = std::fs::metadata(config.join(f)).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "{f} is {mode:o}");
        }
    }
}

/// What an interrupted encrypt or decrypt leaves beside a book (possibly a
/// plain-text copy) is removed when the book next opens in the app.
#[tokio::test]
async fn opening_a_book_removes_conversion_leftovers() {
    let (app, dir) = app("leftovers");
    let path = dir.join("Vault.tallybook");
    app.call("book.create", json!({ "path": path, "company_name": "Vault" })).await.unwrap();
    app.call("book.backup", Value::Null).await.unwrap();
    let plain_copy = std::fs::read_dir(dir.join("Backups")).unwrap().next().unwrap().unwrap().path();
    app.call("book.encrypt", Value::Null).await.unwrap();
    app.call("book.close", Value::Null).await.unwrap();
    let previous = dir.join("Vault.tallybook.previous");
    let converting = dir.join("Vault.tallybook.converting");
    std::fs::copy(&plain_copy, &previous).unwrap();
    std::fs::copy(&plain_copy, &converting).unwrap();
    std::fs::write(dir.join("Vault.tallybook.previous-wal"), b"wal").unwrap();

    // An agent session leaves them (it changes no files)...
    app.set_update_recent(false);
    app.call("book.open", json!({ "path": path })).await.unwrap();
    assert!(previous.exists() && converting.exists());
    // ...the app removes them.
    app.set_update_recent(true);
    app.call("book.open", json!({ "path": path })).await.unwrap();
    assert!(!previous.exists() && !converting.exists() && !dir.join("Vault.tallybook.previous-wal").exists());
    assert!(path.exists(), "the book itself stays");
    assert!(tally_store::crypto::is_encrypted_file(&path).unwrap());
}

/// A call that does several things does all of them or none: an error never
/// hides a half-made change that a retry would then duplicate.
#[tokio::test]
async fn compound_calls_change_nothing_when_they_fail() {
    let (app, dir) = app("atomic");
    app.call("book.create_demo", json!({ "path": dir.join("demo.tallybook") })).await.unwrap();
    app.call("settings.set", json!({ "lock_date": "2020-12-31" })).await.unwrap();
    let count = |v: Value| v.as_array().unwrap().len();

    let accounts = count(app.call("accounts.list", Value::Null).await.unwrap());
    let err = app
        .call("accounts.create", json!({ "name": "Old savings", "subtype": "bank", "opening_balance": 5000, "opening_date": "2020-06-30" }))
        .await
        .unwrap_err();
    assert_eq!(err.code, "rule");
    assert_eq!(count(app.call("accounts.list", Value::Null).await.unwrap()), accounts, "no account without its opening balance");

    let list = app.call("accounts.list", Value::Null).await.unwrap();
    let id = |code: &str| list.as_array().unwrap().iter().find(|a| a["code"] == code).unwrap()["id"].clone();
    let range = json!({ "from": "2000-01-01", "to": "2100-01-01" });
    let entries = count(app.call("entries.list", range.clone()).await.unwrap());
    let err = app
        .call("entries.post", json!({ "date": today(), "memo": "Lunch", "payee_id": 999_999, "lines": [
            { "account_id": id("6250"), "amount": 1500 }, { "account_id": id("4000"), "amount": -1500 }
        ] }))
        .await
        .unwrap_err();
    assert_eq!(err.code, "not_found", "{err:?}");
    assert_eq!(count(app.call("entries.list", range.clone()).await.unwrap()), entries, "no entry without its payee");

    let waiting = app.call("inbox.count", Value::Null).await.unwrap();
    let txn = app.call("inbox.list", Value::Null).await.unwrap()[0]["id"].clone();
    let err = app.call("inbox.accept", json!({ "id": txn, "account_id": id("6250"), "person_id": 999_999 })).await.unwrap_err();
    assert_eq!(err.code, "not_found");
    assert_eq!(app.call("inbox.count", Value::Null).await.unwrap(), waiting, "the transaction is still waiting");
    assert_eq!(count(app.call("entries.list", range).await.unwrap()), entries);
}

#[tokio::test]
async fn encryption_lifecycle_and_new_machine_recovery() {
    let (app, dir) = app("encrypt");
    let path = dir.join("Vault.tallybook");
    app.call("book.create_demo", json!({ "path": path })).await.unwrap();
    app.call("book.backup", Value::Null).await.unwrap(); // a plaintext backup from before
    let before = app.call("reports.trial_balance", Value::Null).await.unwrap();

    let enc = app.call("book.encrypt", Value::Null).await.unwrap();
    let recovery = enc["recovery_key"].as_str().unwrap().to_string();
    assert!(recovery.starts_with("TB1-"));
    assert_eq!(enc["plain_backups"], 1, "the earlier backup is flagged as still readable");
    assert_eq!(app.call("book.status", Value::Null).await.unwrap()["encrypted"], true);
    assert!(tally_store::crypto::is_encrypted_file(&path).unwrap());
    assert_eq!(app.call("reports.trial_balance", Value::Null).await.unwrap(), before, "same books after encrypting");
    assert!(!dir.join("Vault.tallybook.previous").exists() && !dir.join("Vault.tallybook.converting").exists(), "no leftovers");

    // New backups are encrypted; the plaintext one can be removed.
    let b = app.call("book.backup", Value::Null).await.unwrap();
    assert!(tally_store::crypto::is_encrypted_file(&PathBuf::from(b.as_str().unwrap())).unwrap());
    assert_eq!(app.call("book.delete_plain_backups", Value::Null).await.unwrap(), 1);

    // Reopening on this machine needs nothing: the key is in the keychain.
    app.call("book.close", Value::Null).await.unwrap();
    app.call("book.open", json!({ "path": path })).await.unwrap();
    assert_eq!(app.call("book.recovery_key", Value::Null).await.unwrap(), json!(recovery));

    // A different machine (fresh keychain) gets "locked", then the recovery key opens it.
    let (other, _) = self::app("encrypt-other");
    assert_eq!(other.call("book.open", json!({ "path": path })).await.unwrap_err().code, "locked");
    // Change one character in the middle (never the same character).
    let mid = recovery.len() / 2;
    let swap = if recovery.as_bytes()[mid] == b'Q' { "R" } else { "Q" };
    let typo = format!("{}{swap}{}", &recovery[..mid], &recovery[mid + 1..]);
    assert_eq!(other.call("book.unlock", json!({ "path": path, "recovery_key": typo })).await.unwrap_err().code, "rule");
    other.call("book.unlock", json!({ "path": path, "recovery_key": recovery.to_lowercase() })).await.unwrap();
    assert_eq!(other.call("reports.trial_balance", Value::Null).await.unwrap(), before);
    other.call("book.close", Value::Null).await.unwrap();
    other.call("book.open", json!({ "path": path })).await.unwrap(); // remembered now
    // Close it there before converting again: Windows won't replace a file another
    // process has open (book_swap_that_fails_leaves_the_book_open covers that path).
    other.call("book.close", Value::Null).await.unwrap();

    // And back to a plain book.
    app.call("book.decrypt", Value::Null).await.unwrap();
    assert!(!tally_store::crypto::is_encrypted_file(&path).unwrap());
    assert_eq!(app.call("reports.trial_balance", Value::Null).await.unwrap(), before);
}

/// When the converted file can't be swapped in (on Windows, because another
/// Tallybook or an agent has the book open), nothing changes and the book stays
/// open, rather than leaving the app with no book. A directory where the swap
/// parks the original makes the swap fail on every platform.
#[tokio::test]
async fn book_swap_that_fails_leaves_the_book_open() {
    let (app, dir) = app("swap-fails");
    let path = dir.join("Stuck.tallybook");
    app.call("book.create_demo", json!({ "path": path })).await.unwrap();
    let before = app.call("reports.trial_balance", Value::Null).await.unwrap();
    let blocker = PathBuf::from(format!("{}.previous", path.display()));
    std::fs::create_dir_all(blocker.join("in-the-way")).unwrap();

    let err = app.call("book.encrypt", Value::Null).await.unwrap_err();
    assert_eq!(err.code, "io");
    assert!(err.message.contains("nothing was changed"), "{}", err.message);

    assert_eq!(app.call("book.status", Value::Null).await.unwrap()["open"], true, "the book is still open");
    assert_eq!(app.call("reports.trial_balance", Value::Null).await.unwrap(), before, "and unchanged");
    assert!(!tally_store::crypto::is_encrypted_file(&path).unwrap(), "the file on disk is the original");
    assert!(!PathBuf::from(format!("{}.converting", path.display())).exists(), "no half-made copy left behind");
}

/// An agent (a second app on the same machine, sharing its keychain) keeps
/// working after the desktop app encrypts the book underneath it: its entries
/// land in the encrypted book, not in the file that was replaced. Unix only,
/// since Windows refuses the swap while the agent has the book open.
#[cfg(unix)]
#[tokio::test]
async fn an_agent_follows_the_book_when_the_app_encrypts_it() {
    let dir = temp_dir("follow");
    let keychain = Arc::new(MemorySecrets::default());
    // `--mcp` shares the desktop app's config folder and keychain.
    let desktop = App::new(keychain.clone(), dir.join("config")).with_file_remover(|p| std::fs::remove_file(p));
    let agent = App::new(keychain, dir.join("config")).with_file_remover(|p| std::fs::remove_file(p));
    let path = dir.join("Shared.tallybook");
    desktop.call("book.create_demo", json!({ "path": path })).await.unwrap();
    agent.call("book.open", json!({ "path": path })).await.unwrap();

    desktop.call("book.encrypt", Value::Null).await.unwrap();
    assert!(tally_store::crypto::is_encrypted_file(&path).unwrap());

    let accounts = agent.call("accounts.list", Value::Null).await.unwrap();
    let id = |pred: &dyn Fn(&Value) -> bool| accounts.as_array().unwrap().iter().find(|a| pred(a)).unwrap()["id"].clone();
    let bank = id(&|a| a["subtype"] == "bank");
    let spend = id(&|a| a["kind"] == "expense");
    agent
        .call(
            "entries.post",
            json!({ "date": today(), "memo": "Filed by the agent", "lines": [
                { "account_id": spend, "amount": 4200 }, { "account_id": bank, "amount": -4200 }
            ] }),
        )
        .await
        .unwrap();

    let seen = desktop.call("entries.list", json!({ "from": today(), "to": today() })).await.unwrap();
    assert!(
        seen.as_array().unwrap().iter().any(|e| e["memo"] == "Filed by the agent"),
        "the agent's entry reached the encrypted book the desktop app has open"
    );

    // An agent that can't open the new file (no key for it) stops, rather than
    // writing into the replaced one.
    let stranger = App::new(Arc::new(MemorySecrets::default()), dir.join("stranger")).with_file_remover(|p| std::fs::remove_file(p));
    desktop.call("book.decrypt", Value::Null).await.unwrap();
    stranger.call("book.open", json!({ "path": path })).await.unwrap();
    desktop.call("book.encrypt", Value::Null).await.unwrap();
    assert_eq!(stranger.call("accounts.list", Value::Null).await.unwrap_err().code, "no_book");
}

#[tokio::test]
async fn cloud_copies_are_encrypted_pruned_and_pushed() {
    use std::process::Command;
    let (app, dir) = app("cloud");
    app.call("book.create", json!({ "path": dir.join("Shop.tallybook"), "company_name": "Shop" })).await.unwrap();

    let drive = dir.join("Google Drive");
    std::fs::create_dir_all(&drive).unwrap();
    app.call("cloud.set", json!({ "folder": drive, "keep": 2 })).await.unwrap();
    // Plain books never leave the machine.
    assert_eq!(app.call("cloud.backup_now", Value::Null).await.unwrap_err().code, "rule");

    app.call("book.encrypt", Value::Null).await.unwrap();
    for _ in 0..3 {
        let r = app.call("cloud.backup_now", Value::Null).await.unwrap();
        assert_eq!(r["folder"]["ok"], true, "{r}");
        std::thread::sleep(std::time::Duration::from_millis(1100)); // distinct timestamps
    }
    let copies: Vec<_> = std::fs::read_dir(drive.join("Tallybook")).unwrap().filter_map(Result::ok).map(|e| e.path()).collect();
    assert_eq!(copies.len(), 2, "only the newest `keep` copies remain");
    assert!(copies.iter().all(|c| tally_store::crypto::is_encrypted_file(c).unwrap()));

    // A git repo with a bare remote: commits and pushes.
    let git = |args: &[&str], cwd: &std::path::Path| {
        let ok = Command::new("git").args(args).current_dir(cwd).env("GIT_TERMINAL_PROMPT", "0").output().unwrap();
        assert!(ok.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ok.stderr));
    };
    let remote = dir.join("remote.git");
    let repo = dir.join("backups-repo");
    std::fs::create_dir_all(&remote).unwrap();
    git(&["init", "--bare", "-q"], &remote);
    git(&["clone", "-q", remote.to_str().unwrap(), repo.to_str().unwrap()], &dir);
    git(&["config", "user.email", "test@example.com"], &repo);
    git(&["config", "user.name", "Test"], &repo);
    assert_eq!(app.call("cloud.set", json!({ "git": dir.join("not-a-repo") })).await.unwrap_err().code, "rule");
    app.call("cloud.set", json!({ "git": repo, "folder": null })).await.unwrap();
    let r = app.call("cloud.backup_now", Value::Null).await.unwrap();
    assert_eq!(r["git"]["ok"], true, "{r}");
    assert!(r["git"]["detail"].as_str().unwrap().contains("Pushed"));
    assert!(r["folder"].is_null(), "folder destination was cleared");
    let log = Command::new("git").args(["log", "--oneline", "--all"]).current_dir(&remote).output().unwrap();
    assert!(String::from_utf8_lossy(&log.stdout).contains("Tallybook backup"), "commit reached the remote");
    assert!(tally_store::crypto::is_encrypted_file(&repo.join("Shop.tallybook")).unwrap());

    let status = app.call("cloud.status", Value::Null).await.unwrap();
    assert_eq!(status["encrypted"], true);
    assert_eq!(status["git_has_remote"], true);
    assert!(status["last"].is_string() && status["last_error"].is_null());
}

#[tokio::test]
async fn settings_set_accepts_only_known_preference_keys() {
    let (app, dir) = app("prefs-allowlist");
    app.call("book.create", json!({ "path": dir.join("Shop.tallybook"), "company_name": "Shop" })).await.unwrap();
    let before = app.call("cloud.status", Value::Null).await.unwrap();
    for key in ["cloud.git", "cloud.folder", "cloud.keep", "cloud.auto", "cloud.last", "tax.profile", "plaid.user_id", "lock_date", "anything"] {
        let err = app.call("settings.set", json!({ "prefs": { key: "/tmp/x" } })).await.unwrap_err();
        assert_eq!(err.code, "bad_params", "{key}");
    }
    // One unknown key refuses the whole call: the known one isn't written either.
    let err = app.call("settings.set", json!({ "prefs": { "company.email": "a@b.co", "cloud.auto": "1" } })).await.unwrap_err();
    assert_eq!(err.code, "bad_params");
    let prefs = app.call("settings.get", Value::Null).await.unwrap()["prefs"].clone();
    assert!(prefs.get("company.email").is_none() && prefs.get("cloud.auto").is_none(), "{prefs}");
    assert_eq!(app.call("cloud.status", Value::Null).await.unwrap()["auto"], before["auto"]);
    assert_eq!(app.call("cloud.status", Value::Null).await.unwrap()["git"], before["git"]);

    // Every key the Settings screen saves still works.
    for key in crate::settings::PREF_KEYS {
        app.call("settings.set", json!({ "prefs": { *key: "7" } })).await.unwrap();
    }
}

#[tokio::test]
async fn a_folder_always_keeps_the_copy_just_made() {
    let (app, dir) = app("cloud-keep");
    app.call("book.create", json!({ "path": dir.join("Shop.tallybook"), "company_name": "Shop" })).await.unwrap();
    app.call("book.encrypt", Value::Null).await.unwrap();
    let drive = dir.join("Drive");
    std::fs::create_dir_all(&drive).unwrap();
    app.call("cloud.set", json!({ "folder": drive })).await.unwrap();
    // A stored value outside 1..=100, however it got there, reads back as 1.
    app.with(|s| s.set_pref("cloud.keep", Some("0"))).unwrap();
    assert_eq!(app.call("cloud.status", Value::Null).await.unwrap()["keep"], 1);
    for _ in 0..2 {
        let r = app.call("cloud.backup_now", Value::Null).await.unwrap();
        assert_eq!(r["folder"]["ok"], true, "{r}");
        let newest = PathBuf::from(r["folder"]["path"].as_str().unwrap());
        assert!(newest.exists(), "the copy just made is never pruned");
        std::thread::sleep(std::time::Duration::from_millis(1100)); // distinct timestamps
    }
    assert_eq!(std::fs::read_dir(drive.join("Tallybook")).unwrap().count(), 1);
}

/// Cloud copies into a Git repository run none of the repository's hooks and
/// no filesystem monitor it configures.
#[cfg(unix)]
#[tokio::test]
async fn cloud_git_copies_run_no_repository_hooks() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    let (app, dir) = app("cloud-hooks");
    app.call("book.create", json!({ "path": dir.join("Shop.tallybook"), "company_name": "Shop" })).await.unwrap();
    app.call("book.encrypt", Value::Null).await.unwrap();
    let repo = dir.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        let out = Command::new("git").args(args).current_dir(&repo).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "Test"]);
    let marker = dir.join("ran");
    let script = |name: &str| {
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\necho {name} >> '{}'\nexit 0\n", marker.display())).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    };
    for hook in ["pre-commit", "commit-msg", "post-commit", "pre-push"] {
        std::fs::copy(script(hook), repo.join(".git/hooks").join(hook)).unwrap();
        std::fs::set_permissions(repo.join(".git/hooks").join(hook), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    git(&["config", "core.fsmonitor", script("fsmonitor").to_str().unwrap()]);

    app.call("cloud.set", json!({ "git": repo })).await.unwrap();
    let r = app.call("cloud.backup_now", Value::Null).await.unwrap();
    assert_eq!(r["git"]["ok"], true, "{r}");
    let log = Command::new("git").args(["-c", "core.fsmonitor=false", "log", "--oneline"]).current_dir(&repo).output().unwrap();
    assert!(String::from_utf8_lossy(&log.stdout).contains("Tallybook backup"), "the copy was committed");
    assert!(!marker.exists(), "ran: {}", std::fs::read_to_string(&marker).unwrap_or_default());
}

/// The household mode's book-level methods, as the UI calls them: sides,
/// scoped accounts, and scoped reports and dashboard. Amounts are cents.
#[tokio::test]
async fn a_household_book_and_its_sides_work_through_the_dispatcher() {
    let (app, dir) = app("household");
    app.call("book.create", json!({ "path": dir.join("h.tallybook"), "company_name": "The Rivera household", "sides": ["household"] }))
        .await
        .unwrap();
    assert_eq!(app.call("book.status", Value::Null).await.unwrap()["sides"], json!(["household"]));

    // New accounts default to the book's only side.
    let checking = app.call("accounts.create", json!({ "name": "Checking", "subtype": "bank", "opening_balance": 200000, "opening_date": "2026-01-01" })).await.unwrap();
    let accounts = app.call("accounts.list", Value::Null).await.unwrap();
    let mine = accounts.as_array().unwrap().iter().find(|a| a["id"] == checking).unwrap();
    assert_eq!(mine["scope"], "household");
    assert_eq!(mine["balance"], 200000);
    let groceries = accounts.as_array().unwrap().iter().find(|a| a["name"] == "Groceries").unwrap();
    assert_eq!(groceries["flex"], "usage");

    // Business accounts need a business side first.
    let err = app.call("accounts.create", json!({ "name": "Biz", "subtype": "bank", "scope": "business" })).await.unwrap_err();
    assert_eq!(err.code, "rule");
    app.call("book.add_side", json!({ "scope": "business" })).await.unwrap();
    assert_eq!(app.call("book.status", Value::Null).await.unwrap()["sides"], json!(["business", "household"]));
    let biz = app.call("accounts.create", json!({ "name": "Biz", "subtype": "bank", "scope": "business" })).await.unwrap();

    // Paying yourself shows up as household income, and not as business spending.
    app.call(
        "entries.post",
        json!({ "date": "2026-02-01", "memo": "Pay myself", "lines": [
            { "account_id": checking, "amount": 150000 }, { "account_id": biz, "amount": -150000 }
        ] }),
    )
    .await
    .unwrap();
    let home = app.call("reports.pnl", json!({ "from": "2026-01-01", "to": "2026-12-31", "scope": "household" })).await.unwrap();
    assert_eq!(home["total_income"], 150000);
    let business = app.call("reports.pnl", json!({ "from": "2026-01-01", "to": "2026-12-31", "scope": "business" })).await.unwrap();
    assert_eq!(business["net_income"], 0);
    let sheet = app.call("reports.balance_sheet", json!({ "as_of": "2026-12-31", "scope": "household" })).await.unwrap();
    assert_eq!(sheet["total_assets"], 350000);
    let home_dash = app.call("dashboard", json!({ "scope": "household" })).await.unwrap();
    assert_eq!(home_dash["cash"], 350000);
}

/// Cash on the business side: "entries.post" with a `payee_id` (for 1099s).
/// 4000 is Sales and 6150 Contractors in the default chart.
#[tokio::test]
async fn cash_paid_from_the_till_keeps_its_payee_and_moves_the_balance() {
    let (app, dir) = app("till");
    app.call("book.create", json!({ "path": dir.join("t.tallybook"), "company_name": "Shop" })).await.unwrap();
    let till = app.call("accounts.create", json!({ "name": "Cash till", "subtype": "bank", "opening_balance": 20000, "opening_date": "2026-01-01" })).await.unwrap();
    let id = |code: &str, accounts: &Value| accounts.as_array().unwrap().iter().find(|a| a["code"] == code).unwrap()["id"].clone();
    let accounts = app.call("accounts.list", Value::Null).await.unwrap();
    let (sales, contractors) = (id("4000", &accounts), id("6150", &accounts));
    let pat = app.call("contacts.create", json!({ "kind": "vendor", "name": "Pat the painter" })).await.unwrap();

    // A cash sale into the till, then cash out of it to a contractor.
    app.call("entries.post", json!({ "date": "2026-03-07", "memo": "Market sales", "lines": [
        { "account_id": till, "amount": 45000 }, { "account_id": sales, "amount": -45000 } ] })).await.unwrap();
    let paid = app.call("entries.post", json!({ "date": "2026-03-08", "memo": "Touch-up", "payee_id": pat, "lines": [
        { "account_id": contractors, "amount": 30000 }, { "account_id": till, "amount": -30000 } ] })).await.unwrap();

    assert_eq!(app.call("entries.get", json!({ "id": paid })).await.unwrap()["payee_id"], pat);
    let till_now = app.call("accounts.list", Value::Null).await.unwrap().as_array().unwrap().iter().find(|a| a["id"] == till).unwrap()["balance"].clone();
    assert_eq!(till_now, 35000);
}

// ---------------------------------------------------------------- API pass

/// `bank.sync` with no connection named syncs them all, and a connection that
/// can't sync (its credential is gone from the keychain) is reported with
/// `ok: false` instead of discarding the rest.
#[tokio::test]
async fn sync_all_reports_a_failing_connection_and_carries_on() {
    let (app, dir) = app("sync-all");
    app.call("book.create", json!({ "path": dir.join("s.tallybook"), "company_name": "Shop" })).await.unwrap();
    let account = app
        .run(|s| s.create_account(&NewAccount { code: None, name: "Checking".into(), subtype: AccountSubtype::Bank, parent_id: None, scope: Scope::Business, flex: None }))
        .unwrap();
    // The first connection has a link but its secret was never stored; the second has no links, so it syncs trivially.
    let (broken, fine) = app
        .run(|s| {
            let broken = s.create_connection(Provider::Simplefin, "Broken CU", None, Some("simplefin:00000000-0000-4000-8000-000000000002"))?;
            s.link_account(broken, "ext-1", account, "Checking", None, today())?;
            let fine = s.create_connection(Provider::Simplefin, "Fine CU", None, Some("simplefin:00000000-0000-4000-8000-000000000003"))?;
            Ok((broken, fine))
        })
        .unwrap();

    let reports = app.call("bank.sync", json!({})).await.unwrap();
    let reports = reports.as_array().unwrap();
    assert_eq!(reports.len(), 2, "{reports:?}");
    assert_eq!(reports[0]["connection_id"], broken);
    assert_eq!(reports[0]["institution"], "Broken CU");
    assert_eq!(reports[0]["ok"], false);
    assert!(reports[0]["error"].as_str().unwrap().contains("keychain"), "{reports:?}");
    assert_eq!(reports[1]["connection_id"], fine);
    assert_eq!(reports[1]["ok"], true);

    // Naming the broken connection still returns its error as an error.
    assert_eq!(app.call("bank.sync", json!({ "connection_id": broken })).await.unwrap_err().code, "rule");
}

/// A/R and A/P exist only on the business side, so `scope` is honored by
/// emptying the report for any other side.
#[tokio::test]
async fn aging_reports_honor_scope() {
    let (app, dir) = app("aging");
    app.call("book.create", json!({ "path": dir.join("a.tallybook"), "company_name": "Shop" })).await.unwrap();
    let accounts = app.call("accounts.list", Value::Null).await.unwrap();
    let sales = accounts.as_array().unwrap().iter().find(|a| a["code"] == "4000").unwrap()["id"].clone();
    let customer = app.call("contacts.create", json!({ "kind": "customer", "name": "Acme" })).await.unwrap();
    let due = today() - chrono::Days::new(40);
    let invoice = app
        .call("invoices.create", json!({
            "customer_id": customer, "issue_date": due, "due_date": due, "memo": "", "tax_rate_bp": 0,
            "lines": [{ "description": "Work", "quantity_milli": 1000, "unit_price": 50000, "account_id": sales, "taxable": false }]
        }))
        .await
        .unwrap();
    app.call("invoices.finalize", json!({ "id": invoice })).await.unwrap();

    for method in ["reports.ar_aging", "reports.ap_aging"] {
        let all = app.call(method, json!({})).await.unwrap();
        let business = app.call(method, json!({ "scope": "business" })).await.unwrap();
        let household = app.call(method, json!({ "scope": "household" })).await.unwrap();
        assert_eq!(all, business, "{method}: business is the default");
        assert!(household["rows"].as_array().unwrap().is_empty(), "{method}: {household}");
        assert!(household["totals"].as_object().unwrap().values().all(|v| v == 0), "{method}: {household}");
    }
    let ar = app.call("reports.ar_aging", json!({})).await.unwrap();
    assert_eq!(ar["rows"].as_array().unwrap().len(), 1, "{ar}");
    assert_eq!(ar["totals"]["days_31_60"], 50000, "{ar}");
    let household = app.call("reports.ar_aging", json!({ "scope": "household" })).await.unwrap();
    assert!(household["rows"].as_array().unwrap().is_empty(), "household has no invoices: {household}");
}

/// A `tax_year` too big for an i32 is refused up front; it must not wrap into
/// a real year and pass or fail the license gate as that year.
#[tokio::test]
async fn record_payment_refuses_a_tax_year_that_would_wrap() {
    let (app, dir) = app("wrap");
    app.call("book.create", json!({ "path": dir.join("w.tallybook"), "company_name": "Shop" })).await.unwrap();
    // License one supported year; the gate would say "license" if the number wrapped to another.
    let years = tally_core::tax::supported_years();
    let license = crate::license::License {
        tier: crate::license::Tier::Tax,
        key: "K".into(),
        instance_id: "i".into(),
        email: None,
        activated_on: today(),
        tax_years: vec![years[0]],
        earlier: vec![],
    };
    crate::license::save(&dir.join("config"), &license).unwrap();
    let year = years.into_iter().find(|y| app.require_tax_year(*y).is_err()).expect("a supported year the license doesn't cover");
    let wrapped = i64::from(year) + (1_i64 << 32);
    let e = app
        .call("tax.record_payment", json!({ "tax_year": wrapped, "quarter": 1, "jurisdiction": "federal", "date": "2026-04-15", "amount": 100, "account_id": 1 }))
        .await
        .unwrap_err();
    assert_eq!(e.code, "bad_params", "{e:?}");
    assert!(e.message.contains("out of range"), "{e:?}");
}

/// `recurring.due`'s default window runs to the end of next month, across a year end too.
#[test]
fn recurring_due_default_window_is_the_past_week_through_next_month() {
    let d = |y, m, day| NaiveDate::from_ymd_opt(y, m, day).unwrap();
    assert_eq!(crate::recurring::default_due_window(d(2026, 3, 10)).unwrap(), (d(2026, 3, 3), d(2026, 4, 30)));
    assert_eq!(crate::recurring::default_due_window(d(2026, 12, 31)).unwrap(), (d(2026, 12, 24), d(2027, 1, 31)));
    assert_eq!(crate::recurring::default_due_window(d(2027, 1, 31)).unwrap(), (d(2027, 1, 24), d(2027, 2, 28)));
}

/// `METHODS` lists exactly what the routers dispatch: every entry is routed
/// (never `unknown_method`), and every method literal in a router is listed.
#[tokio::test]
async fn the_method_list_matches_the_routers() {
    let (app, _dir) = free_app("methods");
    for m in METHODS {
        // No book is open and no params are sent, so nothing here does any work.
        let e = app.call(m, Value::Null).await.err();
        assert!(e.is_none_or(|e| e.code != "unknown_method"), "{m} is listed but not routed");
    }
    assert_eq!(app.call("nope.nothing", Value::Null).await.unwrap_err().code, "unknown_method");
    let mut seen = std::collections::BTreeSet::new();
    for src in [
        include_str!("session.rs"), include_str!("books.rs"), include_str!("settings.rs"), include_str!("reports.rs"), include_str!("accounts.rs"),
        include_str!("people.rs"), include_str!("recurring.rs"), include_str!("goals.rs"), include_str!("inbox.rs"), include_str!("import.rs"),
        include_str!("rules.rs"), include_str!("reconcile.rs"), include_str!("contacts.rs"), include_str!("invoices.rs"), include_str!("bills.rs"),
        include_str!("tax_api.rs"), include_str!("sync.rs"), include_str!("license.rs"), include_str!("cloud.rs"),
    ] {
        for line in src.lines().map(str::trim).filter(|l| l.starts_with('"') && l.contains("=>")) {
            let arms = line.split("=>").next().unwrap();
            for lit in arms.split('|').map(|a| a.trim().trim_matches('"')) {
                if lit.contains('.') || lit == "dashboard" {
                    seen.insert(lit.to_string());
                }
            }
        }
    }
    for m in &seen {
        assert!(METHODS.contains(&m.as_str()), "{m} is routed but missing from METHODS");
    }
    assert!(seen.len() > 100, "the scan found only {} arms", seen.len());
}
