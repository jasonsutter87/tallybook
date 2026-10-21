use chrono::Datelike;
use super::*;
use serde_json::json;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tallybook-test-{name}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn app(name: &str) -> (App, PathBuf) {
    let dir = temp_dir(name);
    (App::new(Arc::new(MemorySecrets::default()), dir.join("config")).with_file_remover(|p| std::fs::remove_file(p)), dir)
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
    for f in ["README.txt", "Schedule C worksheet.csv", "Schedule C mapping.csv", "General ledger.csv", "1099 summary.csv", "Estimated tax payments.csv"] {
        assert!(out.join(f).exists(), "{f}");
    }
    assert!(std::fs::read_to_string(out.join("README.txt")).unwrap().contains("not tax advice"));

    assert_eq!(app.call("tax.estimate", json!({ "year": 2019 })).await.unwrap_err().code, "unsupported_year");
}

#[tokio::test]
async fn delete_book_tears_everything_down() {
    let (app, dir) = app("delete");
    let path = dir.join("Gone.tallybook");
    app.call("book.create", json!({ "path": path, "company_name": "Gone Co" })).await.unwrap();
    app.call("book.backup", Value::Null).await.unwrap();
    // A bank connection whose credential lives in the keychain.
    app.secrets.set("simplefin:test", "https://user:pass@bridge.example/simplefin").unwrap();
    app.run(|s| s.create_connection(Provider::Simplefin, "CU", None, Some("simplefin:test"))).unwrap();

    let wrong = app.call("book.delete", json!({ "path": path, "confirm_name": "gone co" })).await.unwrap_err();
    assert_eq!(wrong.code, "rule", "the name must match exactly");
    assert!(path.exists());

    let r = app.call("book.delete", json!({ "path": path, "confirm_name": "Gone Co", "include_backups": true })).await.unwrap();
    assert!(!path.exists());
    assert_eq!(r["keychain_entries_removed"], 1);
    assert!(app.secrets.get("simplefin:test").unwrap().is_none(), "credential removed from the keychain");
    assert!(std::fs::read_dir(dir.join("Backups")).unwrap().next().is_none(), "backups removed too");
    assert!(!app.is_open(), "the open book was closed first");
    assert!(app.call("book.recent", Value::Null).await.unwrap().as_array().unwrap().is_empty());

    let not_a_book = dir.join("notes.txt");
    std::fs::write(&not_a_book, "hi").unwrap();
    assert_eq!(app.call("book.delete", json!({ "path": not_a_book, "confirm_name": "x" })).await.unwrap_err().code, "rule");
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

    // And back to a plain book.
    app.call("book.decrypt", Value::Null).await.unwrap();
    assert!(!tally_store::crypto::is_encrypted_file(&path).unwrap());
    assert_eq!(app.call("reports.trial_balance", Value::Null).await.unwrap(), before);
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
async fn household_books_through_the_dispatcher() {
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

#[tokio::test]
async fn cash_from_the_till_with_a_payee() {
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
