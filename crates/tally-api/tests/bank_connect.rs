//! Connecting a bank through Tallybook Connect: who may, what the app sends,
//! and that the app only ever holds Connect's stand-in, never a Plaid token.
//! A fake Connect on 127.0.0.1 plays the server.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tally_api::{App, MemorySecrets};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const KEY: &str = "PERSONAL-KEY-ABCD-1234";
const HANDLE: &str = "tbc-00000000-0000-4000-8000-000000000001";

/// One request the fake Connect saw: path, Authorization header, JSON body.
type Seen = Arc<Mutex<Vec<(String, String, Value)>>>;

/// A fake Connect answering each Plaid path the way the real one would,
/// with the stand-in handle in place of an access token. Returns its URL.
async fn fake_connect() -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen: Seen = Arc::default();
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let log = log.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                // Read the head, then exactly Content-Length bytes of body.
                let (head_end, len) = loop {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..i]).to_lowercase();
                        let len = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
                        break (i + 4, len);
                    }
                };
                while buf.len() < head_end + len {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or_default().to_string();
                let auth = head.lines().find_map(|l| l.to_lowercase().starts_with("authorization:").then(|| l[14..].trim().to_string())).unwrap_or_default();
                let body: Value = serde_json::from_slice(&buf[head_end..]).unwrap_or(Value::Null);
                let reply = match path.trim_start_matches("/v1/plaid/") {
                    "link/token/create" => json!({ "link_token": "link-1", "hosted_link_url": "https://hosted.plaid.example/link-1" }),
                    "link/token/get" => json!({ "link_sessions": [{ "results": { "item_add_results": [{ "public_token": "public-1", "institution": { "name": "First Platypus Bank" } }] } }] }),
                    "item/public_token/exchange" => json!({ "access_token": HANDLE, "item_id": "item-1" }),
                    "accounts/get" => json!({ "accounts": [{ "account_id": "acc-1", "name": "Everyday Checking", "mask": "0001", "type": "depository", "balances": { "current": 120.5, "iso_currency_code": "USD" } }] }),
                    "transactions/sync" => json!({
                        "added": [{ "transaction_id": "t-1", "account_id": "acc-1", "date": "2026-10-01", "amount": 12.5, "name": "Corner Cafe", "pending": false }],
                        "modified": [], "removed": [], "next_cursor": "cursor-1", "has_more": false
                    }),
                    "item/remove" => json!({ "request_id": "r" }),
                    _ => json!({ "error_code": "NOT_FOUND", "error_message": "Unknown endpoint." }),
                };
                log.lock().unwrap().push((path, auth, body));
                let text = reply.to_string();
                let resp = format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}", text.len());
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    });
    (url, seen)
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tally-bank-connect-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("config")).unwrap();
    d
}

/// Writes `license.json` the way activation leaves it.
fn license(config: &Path, tier: &str, active: bool) {
    let l = json!({ "tier": tier, "key": KEY, "instance_id": "i-1", "email": null, "activated_on": "2026-01-02", "tax_years": [], "active": active, "checked_on": "2026-01-02" });
    std::fs::write(config.join("license.json"), l.to_string()).unwrap();
}

async fn household(app: &App, d: &Path) {
    let path = d.join("Home.tallybook");
    app.call("book.create", json!({ "path": path, "company_name": "Home", "sides": ["household"] })).await.unwrap();
}

fn app(d: &Path, connect: Option<String>) -> App {
    App::new(Arc::new(MemorySecrets::default()), d.join("config")).with_file_remover(|p| std::fs::remove_file(p)).with_connect_url(connect)
}

#[tokio::test]
async fn a_free_copy_without_its_own_plaid_keys_is_offered_a_plan() {
    let d = dir("free");
    let (url, seen) = fake_connect().await;
    let app = app(&d, Some(url));
    household(&app, &d).await;
    let err = app.call("bank.plaid_start", Value::Null).await.unwrap_err();
    assert_eq!(err.code, "license");
    assert!(err.message.contains("$20 a year") && err.message.contains("Import a file"), "{}", err.message);
    assert!(seen.lock().unwrap().is_empty(), "nothing is sent without a plan");
}

/// A keychain that can't be reached, like Linux without a secret service.
struct NoKeychain;

impl tally_api::Secrets for NoKeychain {
    fn get(&self, _: &str) -> Result<Option<String>, String> {
        Err("no secret service".into())
    }
    fn set(&self, _: &str, _: &str) -> Result<(), String> {
        Err("no secret service".into())
    }
    fn delete(&self, _: &str) -> Result<(), String> {
        Err("no secret service".into())
    }
}

#[tokio::test]
async fn without_a_keychain_a_free_copy_still_hears_about_the_plan() {
    let d = dir("no-keychain");
    let app = App::new(Arc::new(NoKeychain), d.join("config")).with_file_remover(|p| std::fs::remove_file(p)).with_connect_url(None);
    household(&app, &d).await;
    let err = app.call("bank.plaid_start", Value::Null).await.unwrap_err();
    assert_eq!(err.code, "license", "{}", err.message);
}

#[tokio::test]
async fn an_ended_plan_pauses_new_connections() {
    let d = dir("ended");
    let (url, seen) = fake_connect().await;
    license(&d.join("config"), "personal", false);
    let app = app(&d, Some(url));
    household(&app, &d).await;
    let err = app.call("bank.plaid_start", Value::Null).await.unwrap_err();
    assert_eq!(err.code, "license");
    assert!(err.message.contains("ended"), "{}", err.message);
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn without_a_connect_server_the_app_says_so_plainly() {
    let d = dir("no-connect");
    license(&d.join("config"), "personal", true);
    let app = app(&d, None);
    household(&app, &d).await;
    let err = app.call("bank.plaid_start", Value::Null).await.unwrap_err();
    assert_eq!(err.code, "connect_unavailable");
}

#[tokio::test]
async fn a_plan_connects_and_syncs_through_connect_holding_only_the_stand_in() {
    let d = dir("connect");
    let (url, seen) = fake_connect().await;
    license(&d.join("config"), "personal", true);
    let app = app(&d, Some(url));
    household(&app, &d).await;

    // Start: the hosted link comes back from Connect.
    let link = app.call("bank.plaid_start", Value::Null).await.unwrap();
    assert_eq!(link["url"], "https://hosted.plaid.example/link-1");

    // Finish: the connection is made. What the keychain keeps is Connect's
    // stand-in: it's what the later calls send (checked below).
    let done = app.call("bank.plaid_poll", json!({ "link_token": "link-1", "connection_id": null })).await.unwrap();
    assert_eq!((done["status"].as_str(), done["institution"].as_str()), (Some("linked"), Some("First Platypus Bank")));
    let connection_id = done["connection_id"].as_i64().unwrap();

    // Link the checking account and sync: transactions arrive through Connect.
    app.call(
        "bank.link",
        json!({ "connection_id": connection_id, "external_account_id": "acc-1", "name": "Everyday Checking", "mask": "0001",
                "account_id": null, "new_account": { "name": "Everyday Checking", "subtype": "bank" }, "start_date": "2026-09-01" }),
    )
    .await
    .unwrap();
    let report = app.call("bank.sync", json!({ "connection_id": connection_id })).await.unwrap();
    let report = if report.is_array() { report[0].clone() } else { report };
    assert_eq!(report["ok"], true, "{report}");
    assert_eq!(app.call("inbox.count", Value::Null).await.unwrap(), 1);

    // Every call went to Connect with the license key, and carried no Plaid
    // credentials; calls about the bank carried the stand-in.
    let seen = seen.lock().unwrap().clone();
    let paths: Vec<&str> = seen.iter().map(|(p, _, _)| p.as_str()).collect();
    assert_eq!(
        paths,
        [
            "/v1/plaid/link/token/create",
            "/v1/plaid/link/token/get",
            "/v1/plaid/item/public_token/exchange",
            "/v1/plaid/accounts/get",
            "/v1/plaid/transactions/sync",
            // Balances, after the transactions.
            "/v1/plaid/accounts/get",
        ]
    );
    for (path, auth, body) in seen.iter() {
        assert_eq!(auth, &format!("Bearer {KEY}"), "{path}");
        assert!(body.get("client_id").is_none() && body.get("secret").is_none(), "{path} carried credentials");
    }
    for i in [3, 4, 5] {
        assert_eq!(seen[i].2["access_token"], HANDLE, "{}", seen[i].0);
    }

    // Disconnecting tells Connect, which removes the bank at Plaid.
    app.call("bank.disconnect", json!({ "connection_id": connection_id })).await.unwrap();
}
