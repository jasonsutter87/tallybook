//! Books made by released versions must still open, add up to the same
//! numbers, and take new entries. The fixtures were created by each release's
//! own code; the .json beside each is what that release reported for it. When
//! a schema migration lands, this is what proves it didn't lose anything.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tally_api::{App, MemorySecrets};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// A fresh app with its own copy of `book`, so the fixture itself is never touched.
fn open_copy(book: &str) -> (App, PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("tallybook-upgrade-{book}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{book}.tallybook"));
    std::fs::copy(fixtures().join(format!("{book}.tallybook")), &path).unwrap();
    let app = App::new(Arc::new(MemorySecrets::default()), dir.join("config")).with_file_remover(|p| std::fs::remove_file(p));
    (app, path, dir)
}

fn snapshot(book: &str) -> Value {
    serde_json::from_slice(&std::fs::read(fixtures().join(format!("{book}.json"))).unwrap()).unwrap()
}

async fn call(app: &App, method: &str, params: Value) -> Value {
    app.call(method, params).await.unwrap_or_else(|e| panic!("{method}: {} ({})", e.message, e.code))
}

async fn len(app: &App, method: &str, params: Value) -> usize {
    call(app, method, params).await.as_array().map_or(0, Vec::len)
}

/// Everything the snapshot recorded, read again through today's API.
async fn reread(app: &App, snap: &Value) -> Value {
    let as_of = &snap["as_of"];
    let tb = call(app, "reports.trial_balance", json!({ "as_of": as_of })).await;
    let pnl = call(app, "reports.pnl", json!({ "from": snap["pnl"]["from"], "to": snap["pnl"]["to"] })).await;
    let mut accounts: Vec<Value> =
        call(app, "accounts.list", Value::Null).await.as_array().unwrap().iter().map(|a| json!([a["id"], a["name"], a["balance"]])).collect();
    accounts.sort_by_key(|a| a[0].as_i64());
    let mut now = json!({
        "accounts": accounts,
        "trial_balance": { "total_debits": tb["total_debits"], "total_credits": tb["total_credits"] },
        "pnl": { "from": snap["pnl"]["from"], "to": snap["pnl"]["to"], "total_income": pnl["total_income"], "total_expenses": pnl["total_expenses"], "net_income": pnl["net_income"] },
        "counts": {
            "entries": len(app, "entries.list", json!({ "from": "2000-01-01", "to": "2030-12-31" })).await,
            "contacts": len(app, "contacts.list", Value::Null).await,
            "invoices": len(app, "invoices.list", Value::Null).await,
            "bills": len(app, "bills.list", Value::Null).await,
            "rules": len(app, "rules.list", Value::Null).await,
            "inbox": call(app, "inbox.count", Value::Null).await,
            "recurring": len(app, "recurring.list", Value::Null).await,
            "goals": len(app, "goals.list", Value::Null).await,
            "people": len(app, "people.list", Value::Null).await,
        },
    });
    if snap.get("budget_2026_10").is_some() {
        let month = call(app, "budget.month", json!({ "month": "2026-10" })).await;
        let mut lines: Vec<Value> = month["lines"].as_array().unwrap().iter().map(|l| json!([l["name"], l["amount"], l["progress"]])).collect();
        lines.sort_by_key(|a| a.to_string());
        now["budget_2026_10"] = json!(lines);
    }
    now
}

/// Opens a released book, checks it against its snapshot, writes to it, and reopens it.
async fn upgrade(book: &str) {
    let snap = snapshot(book);
    let (app, path, dir) = open_copy(book);
    call(&app, "book.open", json!({ "path": path })).await;

    // Same numbers as the release that made it.
    let mut expected = snap.clone();
    expected.as_object_mut().unwrap().retain(|k, _| !matches!(k.as_str(), "made_by" | "as_of"));
    if let Some(lines) = expected.get_mut("budget_2026_10") {
        let mut sorted = lines.as_array().unwrap().clone();
        sorted.sort_by_key(|a| a.to_string());
        *lines = json!(sorted);
    }
    assert_eq!(reread(&app, &snap).await, expected, "{book} reads differently than {} reported", snap["made_by"]);

    // Still writable: a balanced entry posts, and voiding it keeps the books balanced.
    let accounts = call(&app, "accounts.list", Value::Null).await;
    let bank = accounts.as_array().unwrap().iter().find(|a| a["subtype"] == "bank").expect("a bank account");
    let spend = accounts.as_array().unwrap().iter().find(|a| a["kind"] == "expense" && a["scope"] == bank["scope"]).expect("an expense account");
    let entry = call(
        &app,
        "entries.post",
        json!({ "date": "2026-10-23", "memo": "Upgrade check", "lines": [
            { "account_id": spend["id"], "amount": 1234 }, { "account_id": bank["id"], "amount": -1234 }
        ] }),
    )
    .await;
    call(&app, "entries.void", json!({ "id": entry, "date": "2026-10-23" })).await;

    // And it all survives closing and reopening.
    call(&app, "book.close", Value::Null).await;
    call(&app, "book.open", json!({ "path": path })).await;
    let tb = call(&app, "reports.trial_balance", json!({ "as_of": "2026-12-31" })).await;
    assert_eq!(tb["total_debits"], tb["total_credits"], "{book} out of balance after reopening");
    let entries = len(&app, "entries.list", json!({ "from": "2000-01-01", "to": "2030-12-31" })).await;
    assert_eq!(entries, snap["counts"]["entries"].as_u64().unwrap() as usize + 2, "the new entry and its void were kept");

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_business_book_from_v0_2_0_opens_adds_up_and_takes_entries() {
    upgrade("business-v0.2.0").await;
}

#[tokio::test]
async fn a_household_book_from_v0_2_0_opens_adds_up_and_takes_entries() {
    upgrade("household-v0.2.0").await;
}
