//! End-to-end tests: a fake Plaid and a fake Lemon Squeezy on local ports,
//! the real Connect server, and (where possible) the real `tally-feed` client
//! in Connect mode.

use axum::extract::{Form, State};
use axum::http::{StatusCode, Uri};
use axum::response::IntoResponse;
use axum::{Json, Router};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tally_connect::config::Config;
use tally_connect::license::sha256_hex;
use tally_connect::sweep::{SweepReport, sweep_once};
use tally_connect::{AppState, proxy};
use tally_feed::plaid::{self, Client, PlaidError};

const STORE: &str = "4242";
const PRODUCT: &str = "777";
const CID: &str = "our-client-id";
const SECRET: &str = "our-plaid-secret";
const DAY: i64 = 86_400;

// ---------- fake Plaid ----------

#[derive(Default)]
struct FakePlaid {
    calls: Mutex<Vec<(String, Value)>>,
    minted: AtomicUsize,
    remove_fails: AtomicBool,
}

impl FakePlaid {
    fn calls_to(&self, path: &str) -> Vec<Value> {
        self.calls.lock().unwrap().iter().filter(|(p, _)| p == path).map(|(_, b)| b.clone()).collect()
    }
}

async fn plaid_handler(State(p): State<Arc<FakePlaid>>, uri: Uri, Json(body): Json<Value>) -> impl IntoResponse {
    let path = uri.path().to_string();
    p.calls.lock().unwrap().push((path.clone(), body.clone()));
    if body["client_id"] != CID || body["secret"] != SECRET {
        return (StatusCode::BAD_REQUEST, Json(json!({"error_type": "INVALID_INPUT", "error_code": "INVALID_API_KEYS", "error_message": "bad keys"})));
    }
    let real_token = |b: &Value| b["access_token"].as_str().is_some_and(|t| t.starts_with("access-sandbox-REAL"));
    match path.as_str() {
        "/link/token/create" => (StatusCode::OK, Json(json!({"link_token": "link-1", "hosted_link_url": "https://hosted.plaid.test/x", "request_id": "r"}))),
        "/link/token/get" => (StatusCode::OK, Json(json!({"link_sessions": []}))),
        "/item/public_token/exchange" => {
            let n = p.minted.fetch_add(1, Ordering::SeqCst) + 1;
            (StatusCode::OK, Json(json!({"access_token": format!("access-sandbox-REAL-{n}"), "item_id": format!("item-{n}"), "request_id": "r"})))
        }
        "/accounts/get" | "/transactions/sync" if !real_token(&body) => {
            (StatusCode::BAD_REQUEST, Json(json!({"error_type": "INVALID_INPUT", "error_code": "INVALID_ACCESS_TOKEN", "error_message": "bad token"})))
        }
        "/accounts/get" => (StatusCode::OK, Json(json!({"accounts": [{"account_id": "a1", "name": "Checking", "type": "depository", "balances": {"current": 5.0}}]}))),
        "/transactions/sync" => (StatusCode::OK, Json(json!({"added": [], "modified": [], "removed": [], "next_cursor": "c", "has_more": false}))),
        "/item/remove" if p.remove_fails.load(Ordering::SeqCst) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error_code": "INTERNAL_SERVER_ERROR", "error_type": "API_ERROR"}))),
        "/item/remove" => (StatusCode::OK, Json(json!({"request_id": "r"}))),
        _ => (StatusCode::NOT_FOUND, Json(json!({"error_code": "NOT_FOUND"}))),
    }
}

// ---------- fake Lemon Squeezy ----------

#[derive(Default)]
struct FakeLs {
    keys: Mutex<HashMap<String, Value>>,
    hits: AtomicUsize,
    down: AtomicBool,
}

fn ls_reply(valid: bool, status: &str, store: &str, product: &str) -> Value {
    json!({"valid": valid, "error": null, "license_key": {"status": status},
           "meta": {"store_id": store.parse::<i64>().unwrap(), "product_id": product.parse::<i64>().unwrap()}})
}

async fn ls_handler(State(l): State<Arc<FakeLs>>, Form(form): Form<HashMap<String, String>>) -> impl IntoResponse {
    l.hits.fetch_add(1, Ordering::SeqCst);
    if l.down.load(Ordering::SeqCst) {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": "down"})));
    }
    match l.keys.lock().unwrap().get(form.get("license_key").map(String::as_str).unwrap_or("")) {
        Some(v) => (StatusCode::OK, Json(v.clone())),
        None => (StatusCode::NOT_FOUND, Json(json!({"valid": false, "error": "license_key not found.", "license_key": null, "meta": null}))),
    }
}

async fn spawn(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
    url
}

// ---------- harness ----------

struct Harness {
    url: String,
    plaid: Arc<FakePlaid>,
    ls: Arc<FakeLs>,
    state: Arc<AppState>,
    logs: Arc<Mutex<Vec<String>>>,
    db_path: std::path::PathBuf,
}

impl Drop for Harness {
    fn drop(&mut self) {
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", self.db_path.display()));
        }
    }
}

impl Harness {
    async fn new() -> Harness {
        Harness::with(|_| {}).await
    }

    async fn with(tweak: impl FnOnce(&mut Config)) -> Harness {
        let plaid = Arc::new(FakePlaid::default());
        let ls = Arc::new(FakeLs::default());
        let plaid_url = spawn(Router::new().fallback(plaid_handler).with_state(plaid.clone())).await;
        let ls_url = spawn(Router::new().fallback(ls_handler).with_state(ls.clone())).await;
        let db_path = std::env::temp_dir().join(format!("tc-it-{}.db", uuid::Uuid::new_v4()));
        let mut cfg = Config::from_lookup(|k| {
            let v = match k {
                "CONNECT_DB" => db_path.to_str().unwrap(),
                "CONNECT_DB_KEY" => "test-db-key",
                "PLAID_CLIENT_ID" => CID,
                "PLAID_SECRET" => SECRET,
                "PLAID_BASE_URL" => &plaid_url,
                "LS_BASE_URL" => &ls_url,
                "LS_STORE_ID" => STORE,
                "LS_PRODUCT_IDS" => "777, 888",
                _ => return None,
            };
            Some(v.to_string())
        })
        .unwrap();
        tweak(&mut cfg);
        let logs: Arc<Mutex<Vec<String>>> = Default::default();
        let sink = logs.clone();
        let state = AppState::new(cfg, Arc::new(move |l: &str| sink.lock().unwrap().push(l.to_string()))).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(proxy::serve(state.clone(), listener, std::future::pending()));
        Harness { url, plaid, ls, state, logs, db_path }
    }

    fn add_key(&self, key: &str, reply: Value) {
        self.ls.keys.lock().unwrap().insert(key.to_string(), reply);
    }

    fn add_good_key(&self, key: &str) {
        self.add_key(key, ls_reply(true, "active", STORE, PRODUCT));
    }

    fn client(&self, key: &str) -> Client {
        Client::via_connect(&self.url, key)
    }

    async fn post(&self, path: &str, key: Option<&str>, body: Value) -> (u16, Value) {
        let mut req = reqwest::Client::new().post(format!("{}{path}", self.url)).json(&body);
        if let Some(k) = key {
            req = req.bearer_auth(k);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status().as_u16();
        (status, resp.json().await.unwrap_or(Value::Null))
    }

    /// Links one bank through the real client; returns the handle.
    async fn link(&self, key: &str) -> String {
        self.client(key).exchange_public_token("public-sandbox-1").await.unwrap().access_token
    }
}

fn api_code(e: &PlaidError) -> String {
    match e {
        PlaidError::Api { code, .. } => code.clone(),
        other => panic!("expected an API error, got {other}"),
    }
}

fn api_message(e: &PlaidError) -> String {
    e.to_string()
}

// ---------- auth ----------

#[tokio::test]
async fn missing_key_is_401() {
    let h = Harness::new().await;
    let (status, body) = h.post("/v1/plaid/accounts/get", None, json!({})).await;
    assert_eq!(status, 401);
    assert_eq!(body["error_code"], "LICENSE_MISSING");
    assert!(body["display_message"].as_str().unwrap().contains("license key"));
    assert_eq!(h.ls.hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn unknown_wrong_store_and_wrong_product_keys_are_401() {
    let h = Harness::new().await;
    h.add_key("wrong-store", ls_reply(true, "active", "1", PRODUCT));
    h.add_key("wrong-product", ls_reply(true, "active", STORE, "999"));
    h.add_key("inactive", ls_reply(true, "inactive", STORE, PRODUCT));
    for key in ["no-such-key", "wrong-store", "wrong-product", "inactive"] {
        let err = h.client(key).accounts("tbc-x").await.err().unwrap();
        assert_eq!(api_code(&err), "LICENSE_INVALID", "{key}");
        assert!(api_message(&err).contains("includes bank sync"), "{key}: {err}");
        let (status, _) = h.post("/v1/plaid/accounts/get", Some(key), json!({})).await;
        assert_eq!(status, 401, "{key}");
    }
}

#[tokio::test]
async fn expired_key_gets_a_readable_403() {
    let h = Harness::new().await;
    h.add_key("old", ls_reply(false, "expired", STORE, PRODUCT));
    h.add_key("off", ls_reply(false, "disabled", STORE, PRODUCT));
    for key in ["old", "off"] {
        let (status, body) = h.post("/v1/plaid/accounts/get", Some(key), json!({})).await;
        assert_eq!(status, 403);
        assert_eq!(body["error_message"], "Your Tallybook plan has ended. Renew to keep your banks syncing.");
    }
    let err = h.client("old").accounts("tbc-x").await.err().unwrap();
    assert!(api_message(&err).contains("plan has ended"), "{err}");
}

#[tokio::test]
async fn lemon_squeezy_is_asked_once_for_repeated_calls() {
    let h = Harness::new().await;
    h.add_good_key("good");
    h.add_key("bad", ls_reply(true, "inactive", STORE, PRODUCT));
    let c = h.client("good");
    for _ in 0..3 {
        c.link_status("link-1").await.unwrap();
    }
    assert_eq!(h.ls.hits.load(Ordering::SeqCst), 1);
    for _ in 0..3 {
        h.client("bad").link_status("link-1").await.err().unwrap();
    }
    assert_eq!(h.ls.hits.load(Ordering::SeqCst), 2, "invalid results are cached too");
}

#[tokio::test]
async fn lemon_squeezy_outage_is_503_and_not_cached() {
    let h = Harness::new().await;
    h.add_good_key("good");
    h.ls.down.store(true, Ordering::SeqCst);
    let (status, _) = h.post("/v1/plaid/link/token/get", Some("good"), json!({})).await;
    assert_eq!(status, 503);
    h.ls.down.store(false, Ordering::SeqCst);
    let (status, _) = h.post("/v1/plaid/link/token/get", Some("good"), json!({"link_token": "x"})).await;
    assert_eq!(status, 200);
}

// ---------- proxy ----------

#[tokio::test]
async fn only_allowlisted_plaid_paths_are_reachable() {
    let h = Harness::new().await;
    h.add_good_key("good");
    for path in ["payment_initiation/payment/create", "item/access_token/invalidate", "link/token/create/", "accounts/balance/get", "sandbox/item/reset_login", "..%2Fadmin"] {
        let (status, _) = h.post(&format!("/v1/plaid/{path}"), Some("good"), json!({})).await;
        assert_eq!(status, 404, "{path}");
    }
    assert!(h.plaid.calls.lock().unwrap().is_empty(), "nothing reached Plaid");
    let (status, _) = h.post("/v1/plaid/link/token/get", Some("good"), json!({"link_token": "x"})).await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn client_credentials_are_stripped_and_ours_injected() {
    let h = Harness::new().await;
    h.add_good_key("good");
    let (status, _) = h.post("/v1/plaid/link/token/get", Some("good"), json!({"link_token": "x", "client_id": "evil-id", "secret": "evil-secret"})).await;
    assert_eq!(status, 200, "Plaid accepted our keys, so the evil ones were replaced");
    let seen = h.plaid.calls_to("/link/token/get");
    assert_eq!(seen[0]["client_id"], CID);
    assert_eq!(seen[0]["secret"], SECRET);
    // The real client sends no credentials at all in Connect mode.
    h.client("good").link_status("link-2").await.unwrap();
    assert_eq!(h.plaid.calls_to("/link/token/get").len(), 2);
}

#[tokio::test]
async fn client_user_id_is_forced_and_differs_per_license() {
    let h = Harness::new().await;
    h.add_good_key("alice");
    h.add_good_key("bob");
    h.client("alice").create_hosted_link("someone-elses-id", None).await.unwrap();
    h.client("alice").create_hosted_link("another", None).await.unwrap();
    h.client("bob").create_hosted_link("someone-elses-id", None).await.unwrap();
    let seen = h.plaid.calls_to("/link/token/create");
    let ids: Vec<&str> = seen.iter().map(|b| b["user"]["client_user_id"].as_str().unwrap()).collect();
    assert!(ids.iter().all(|i| i.starts_with("tbu-")), "{ids:?}");
    assert_eq!(ids[0], ids[1], "stable for a license");
    assert_ne!(ids[0], ids[2], "different per license");
    assert!(!ids.contains(&"someone-elses-id"));
    assert_eq!(seen[0]["products"], json!(["transactions"]));
    // A request with no user object at all still gets one.
    h.post("/v1/plaid/link/token/create", Some("alice"), json!({"client_name": "x"})).await;
    assert_eq!(h.plaid.calls_to("/link/token/create")[3]["user"]["client_user_id"], ids[0]);
}

#[tokio::test]
async fn exchange_returns_a_handle_and_the_real_token_stays_on_the_server() {
    let h = Harness::new().await;
    h.add_good_key("good");
    let item = h.client("good").exchange_public_token("public-sandbox-1").await.unwrap();
    assert!(item.access_token.starts_with("tbc-"), "{}", item.access_token);
    assert!(!item.access_token.contains("REAL"));
    assert_eq!(item.item_id, "item-1");

    // The handle works anywhere a token would, and Plaid saw the real one.
    let accounts = h.client("good").accounts(&item.access_token).await.unwrap();
    assert_eq!(accounts[0].name, "Checking");
    h.client("good").sync(&item.access_token, "").await.unwrap();
    assert_eq!(h.plaid.calls_to("/accounts/get")[0]["access_token"], "access-sandbox-REAL-1");
    assert_eq!(h.plaid.calls_to("/transactions/sync")[0]["access_token"], "access-sandbox-REAL-1");

    // Raw check of everything the client got back from the exchange.
    let (_, raw) = h.post("/v1/plaid/item/public_token/exchange", Some("good"), json!({"public_token": "p"})).await;
    assert!(!raw.to_string().contains("REAL"), "{raw}");
    assert_eq!(h.state.db.count_items(&sha256_hex("good")).unwrap(), 2);
}

#[tokio::test]
async fn handles_belong_to_one_license() {
    let h = Harness::new().await;
    h.add_good_key("alice");
    h.add_good_key("bob");
    let alices = h.link("alice").await;
    let err = h.client("bob").accounts(&alices).await.err().unwrap();
    assert_eq!(api_code(&err), "UNKNOWN_CONNECTION");
    let err = h.client("bob").remove_item(&alices).await.err().unwrap();
    assert_eq!(api_code(&err), "UNKNOWN_CONNECTION");
    let err = h.client("alice").accounts("tbc-not-a-real-handle").await.err().unwrap();
    assert_eq!(api_code(&err), "UNKNOWN_CONNECTION");
    // A real Plaid token pasted in is not a handle either.
    let (status, _) = h.post("/v1/plaid/accounts/get", Some("alice"), json!({"access_token": "access-sandbox-REAL-1"})).await;
    assert_eq!(status, 404);
    h.client("alice").accounts(&alices).await.unwrap();
    assert!(h.plaid.calls_to("/item/remove").is_empty(), "bob's attempt never reached Plaid");
}

#[tokio::test]
async fn item_cap_blocks_new_links_but_not_update_mode() {
    let h = Harness::with(|c| c.max_items_per_license = 2).await;
    h.add_good_key("good");
    let first = h.link("good").await;
    h.link("good").await;
    let err = h.client("good").create_hosted_link("u", None).await.err().unwrap();
    assert_eq!(api_code(&err), "ITEM_LIMIT_REACHED");
    assert!(api_message(&err).contains("up to 2 connected banks"), "{err}");
    // Update mode (an existing handle) is fine at the cap.
    h.client("good").create_hosted_link("u", Some(&first)).await.unwrap();
    let update = h.plaid.calls_to("/link/token/create").pop().unwrap();
    assert_eq!(update["access_token"], "access-sandbox-REAL-1");
    assert!(update.get("products").is_none());

    // A link started earlier can't be exchanged past the cap either, and the
    // surplus Item is removed at Plaid rather than left billing us.
    let err = h.client("good").exchange_public_token("public-late").await.err().unwrap();
    assert_eq!(api_code(&err), "ITEM_LIMIT_REACHED");
    assert_eq!(h.plaid.calls_to("/item/remove")[0]["access_token"], "access-sandbox-REAL-3");
    assert_eq!(h.state.db.count_items(&sha256_hex("good")).unwrap(), 2);
    // Another license has its own allowance.
    h.add_good_key("other");
    h.link("other").await;
}

#[tokio::test]
async fn remove_deletes_the_row_and_the_handle_stops_working() {
    let h = Harness::new().await;
    h.add_good_key("good");
    let handle = h.link("good").await;
    assert_eq!(h.state.db.count_items(&sha256_hex("good")).unwrap(), 1);
    h.client("good").remove_item(&handle).await.unwrap();
    assert_eq!(h.plaid.calls_to("/item/remove")[0]["access_token"], "access-sandbox-REAL-1");
    assert_eq!(h.state.db.count_items(&sha256_hex("good")).unwrap(), 0);
    let err = h.client("good").accounts(&handle).await.err().unwrap();
    assert_eq!(api_code(&err), "UNKNOWN_CONNECTION");
}

#[tokio::test]
async fn failed_remove_keeps_the_row() {
    let h = Harness::new().await;
    h.add_good_key("good");
    let handle = h.link("good").await;
    h.plaid.remove_fails.store(true, Ordering::SeqCst);
    assert!(h.client("good").remove_item(&handle).await.is_err());
    assert_eq!(h.state.db.count_items(&sha256_hex("good")).unwrap(), 1);
}

#[tokio::test]
async fn plaid_errors_pass_through_unchanged() {
    let h = Harness::new().await;
    h.add_good_key("good");
    // A well-formed handle owned by the license that Plaid itself rejects:
    // simulate by storing a bogus real token.
    h.state.db.insert_item_capped("tbc-bogus", &sha256_hex("good"), "i", "not-real", 1, 5).unwrap();
    let (status, body) = h.post("/v1/plaid/accounts/get", Some("good"), json!({"access_token": "tbc-bogus"})).await;
    assert_eq!(status, 400);
    assert_eq!(body["error_code"], "INVALID_ACCESS_TOKEN");
    assert_eq!(body["error_message"], "bad token");
}

#[tokio::test]
async fn bring_your_own_credentials_mode_still_works() {
    let h = Harness::new().await;
    // Talk to the fake Plaid directly, as a self-hoster would.
    let plaid_url = h.state.cfg.plaid_base.clone();
    let creds = plaid::Credentials { client_id: CID.into(), secret: SECRET.into(), environment: plaid::Environment::Sandbox };
    let direct = Client::new(creds).with_base_url(plaid_url);
    let item = direct.exchange_public_token("p").await.unwrap();
    assert!(item.access_token.starts_with("access-sandbox-REAL"), "no handle in direct mode");
    assert_eq!(direct.accounts(&item.access_token).await.unwrap().len(), 1);
    let wrong = Client::new(plaid::Credentials { client_id: "x".into(), secret: "y".into(), environment: plaid::Environment::Sandbox }).with_base_url(h.state.cfg.plaid_base.clone());
    assert_eq!(api_code(&wrong.accounts(&item.access_token).await.err().unwrap()), "INVALID_API_KEYS");
}

// ---------- limits ----------

#[tokio::test]
async fn per_license_rate_limit() {
    let h = Harness::with(|c| c.rate_limit_per_min = 3).await;
    h.add_good_key("busy");
    h.add_good_key("quiet");
    for _ in 0..3 {
        h.client("busy").link_status("l").await.unwrap();
    }
    let (status, body) = h.post("/v1/plaid/link/token/get", Some("busy"), json!({"link_token": "l"})).await;
    assert_eq!(status, 429);
    assert_eq!(body["error_code"], "RATE_LIMITED");
    h.client("quiet").link_status("l").await.unwrap();
}

#[tokio::test]
async fn repeated_failed_auth_from_one_address_is_cut_off_without_asking_lemon_squeezy() {
    let h = Harness::new().await;
    h.add_good_key("good");
    let mut last = 0;
    for i in 0..30 {
        let (status, _) = h.post("/v1/plaid/link/token/get", Some(&format!("guess-{i}")), json!({})).await;
        last = status;
    }
    assert_eq!(last, 429);
    assert!(h.ls.hits.load(Ordering::SeqCst) <= 20, "hits: {}", h.ls.hits.load(Ordering::SeqCst));
    // Even a good key is held off from this address for the minute.
    let (status, _) = h.post("/v1/plaid/link/token/get", Some("good"), json!({})).await;
    assert_eq!(status, 429);
}

#[tokio::test]
async fn bodies_over_64kb_are_refused_and_healthz_answers() {
    let h = Harness::new().await;
    h.add_good_key("good");
    let big = "x".repeat(70 * 1024);
    let resp = reqwest::Client::new().post(format!("{}/v1/plaid/link/token/get", h.url)).bearer_auth("good").body(big).send().await.unwrap();
    assert_eq!(resp.status().as_u16(), 413);
    let ok = reqwest::get(format!("{}/healthz", h.url)).await.unwrap();
    assert_eq!(ok.status().as_u16(), 200);
}

// ---------- logs ----------

#[tokio::test]
async fn logs_never_contain_keys_tokens_handles_or_bodies() {
    let h = Harness::new().await;
    let key = "super-secret-license-key-123";
    h.add_good_key(key);
    h.add_key("bad-key-456", ls_reply(false, "expired", STORE, PRODUCT));
    let handle = h.link(key).await;
    h.client(key).accounts(&handle).await.unwrap();
    h.client(key).sync(&handle, "").await.unwrap();
    h.client("bad-key-456").accounts(&handle).await.err().unwrap();
    h.post(&format!("/v1/plaid/{key}"), Some(key), json!({"secret": "body-secret-789"})).await;
    h.post("/v1/plaid/accounts/get", Some(key), json!({"access_token": "tbc-unknown-handle", "note": "body-secret-789"})).await;
    h.client(key).remove_item(&handle).await.unwrap();
    sweep_once(&h.state, chrono_now()).await;

    let logs = h.logs.lock().unwrap().join("\n");
    assert!(logs.contains("POST /v1/plaid/accounts/get 200"), "{logs}");
    assert!(logs.contains(&format!("lic={}", &sha256_hex(key)[..8])));
    for secret in [key, "bad-key-456", handle.as_str(), "access-sandbox-REAL", "tbc-", "body-secret-789", SECRET, "unknown-handle"] {
        assert!(!logs.contains(secret), "log leaked {secret:?}:\n{logs}");
    }
}

// ---------- sweeper ----------

fn chrono_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

#[tokio::test]
async fn sweeper_keeps_items_within_grace_then_removes_them() {
    let h = Harness::new().await;
    h.add_good_key("leaving");
    h.add_good_key("staying");
    h.link("leaving").await;
    h.link("leaving").await;
    h.link("staying").await;
    let (leaving, staying) = (sha256_hex("leaving"), sha256_hex("staying"));
    let t0 = chrono_now();

    // Still active: nothing happens.
    let r = sweep_once(&h.state, t0).await;
    assert_eq!(r, SweepReport { licenses_checked: 2, ..Default::default() });

    // The plan lapses. The first sighting starts the clock; grace keeps items.
    h.add_key("leaving", ls_reply(false, "expired", STORE, PRODUCT));
    let r = sweep_once(&h.state, t0 + DAY).await;
    assert_eq!((r.licenses_in_grace, r.items_removed), (1, 0));
    let r = sweep_once(&h.state, t0 + 20 * DAY).await;
    assert_eq!((r.licenses_in_grace, r.items_removed), (1, 0));
    assert_eq!(h.state.db.count_items(&leaving).unwrap(), 2);
    assert!(h.plaid.calls_to("/item/remove").is_empty());

    // Past the grace period, its items are removed at Plaid and locally.
    let r = sweep_once(&h.state, t0 + 32 * DAY).await;
    assert_eq!(r.items_removed, 2);
    let removed: Vec<String> = h.plaid.calls_to("/item/remove").iter().map(|b| b["access_token"].as_str().unwrap().to_string()).collect();
    assert_eq!(removed, ["access-sandbox-REAL-1", "access-sandbox-REAL-2"]);
    assert_eq!(h.state.db.count_items(&leaving).unwrap(), 0);
    assert_eq!(h.state.db.count_items(&staying).unwrap(), 1, "other licenses untouched");
}

#[tokio::test]
async fn renewing_during_grace_resets_the_clock() {
    let h = Harness::new().await;
    h.add_good_key("k");
    h.link("k").await;
    let hash = sha256_hex("k");
    let t0 = chrono_now();
    h.add_key("k", ls_reply(false, "disabled", STORE, PRODUCT));
    sweep_once(&h.state, t0).await;
    assert_eq!(h.state.db.invalid_since(&hash).unwrap(), Some(t0));
    h.add_good_key("k");
    sweep_once(&h.state, t0 + 10 * DAY).await;
    assert_eq!(h.state.db.invalid_since(&hash).unwrap(), None);
    h.add_key("k", ls_reply(false, "expired", STORE, PRODUCT));
    let r = sweep_once(&h.state, t0 + 40 * DAY).await;
    assert_eq!((r.licenses_in_grace, r.items_removed), (1, 0), "a fresh lapse gets a fresh grace period");
}

#[tokio::test]
async fn sweeper_removes_nothing_on_network_errors_or_surprises() {
    let h = Harness::new().await;
    h.add_good_key("k");
    h.link("k").await;
    let hash = sha256_hex("k");
    let t0 = chrono_now();
    h.add_key("k", ls_reply(false, "expired", STORE, PRODUCT));
    sweep_once(&h.state, t0).await;
    let late = t0 + 90 * DAY;

    // Lemon Squeezy down.
    h.ls.down.store(true, Ordering::SeqCst);
    assert_eq!(sweep_once(&h.state, late).await.skipped, 1);
    h.ls.down.store(false, Ordering::SeqCst);
    // An answer we don't understand, an unknown key, another store's key.
    for reply in [json!({"hello": "world"}), json!({"valid": false, "error": "license_key not found."}), ls_reply(false, "expired", "1", PRODUCT)] {
        h.add_key("k", reply);
        assert_eq!(sweep_once(&h.state, late).await.items_removed, 0);
    }
    // Unreachable Lemon Squeezy (nothing listening).
    let dead = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}", l.local_addr().unwrap())
    };
    let mut cfg = h.state.cfg.clone();
    cfg.ls_base = dead;
    let st2 = AppState::new(cfg, Arc::new(|_| {})).unwrap();
    assert_eq!(sweep_once(&st2, late).await.items_removed, 0);

    assert_eq!(h.state.db.count_items(&hash).unwrap(), 1);
    assert!(h.plaid.calls_to("/item/remove").is_empty());

    // Lapsed past grace, but Plaid fails: keep the row and retry next time.
    h.add_key("k", ls_reply(false, "expired", STORE, PRODUCT));
    h.plaid.remove_fails.store(true, Ordering::SeqCst);
    assert_eq!(sweep_once(&h.state, late).await.items_removed, 0);
    assert_eq!(h.state.db.count_items(&hash).unwrap(), 1);
    h.plaid.remove_fails.store(false, Ordering::SeqCst);
    assert_eq!(sweep_once(&h.state, late + DAY).await.items_removed, 1);
    assert_eq!(h.state.db.count_items(&hash).unwrap(), 0);
}
