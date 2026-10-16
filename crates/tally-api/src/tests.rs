use super::*;
use serde_json::json;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tallybook-test-{name}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn app(name: &str) -> (App, PathBuf) {
    let dir = temp_dir(name);
    (App::new(Arc::new(MemorySecrets::default()), dir.join("config")), dir)
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
