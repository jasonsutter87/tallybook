//! The HTTP surface: auth, the Plaid allowlist, and handle translation.

use crate::AppState;
use crate::license::{Verdict, sha256_hex};
use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, Request, State};
use axum::http::{HeaderMap, StatusCode, header::AUTHORIZATION};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

pub const MAX_BODY_BYTES: usize = 64 * 1024;

/// The only Plaid endpoints a client may reach.
pub const ALLOWED: [&str; 6] = [
    "link/token/create",
    "link/token/get",
    "item/public_token/exchange",
    "accounts/get",
    "transactions/sync",
    "item/remove",
];

const MAX_KEY_LEN: usize = 256;
const HANDLE_PREFIX: &str = "tbc-";

/// A readable error in Plaid's own error shape, so `tally-feed`'s existing
/// error handling shows `display_message` to the user unchanged.
pub struct ApiErr {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiErr {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> ApiErr {
        ApiErr { status, code, message: message.into() }
    }
}

impl IntoResponse for ApiErr {
    fn into_response(self) -> Response {
        let body = json!({
            "error_type": "TALLYBOOK_CONNECT",
            "error_code": self.code,
            "error_message": self.message,
            "display_message": self.message,
        });
        (self.status, Json(body)).into_response()
    }
}

fn internal() -> ApiErr {
    ApiErr::new(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", "Tallybook Connect hit an error. Try again in a moment.")
}

fn plaid_unreachable() -> ApiErr {
    ApiErr::new(StatusCode::BAD_GATEWAY, "BANK_SERVICE_UNAVAILABLE", "Tallybook Connect couldn't reach the bank service. Try again in a moment.")
}

/// Short, non-reversible tag for log lines: the first 8 characters of the
/// license's hash.
#[derive(Clone)]
struct LicenseTag(String);

struct License {
    hash: String,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/v1/plaid/{*path}", post(plaid_route))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(state.clone(), log_requests))
        .with_state(state)
}

/// Serve until `shutdown` resolves, then finish in-flight requests.
pub async fn serve(state: Arc<AppState>, listener: tokio::net::TcpListener, shutdown: impl Future<Output = ()> + Send + 'static) -> std::io::Result<()> {
    axum::serve(listener, router(state).into_make_service_with_connect_info::<SocketAddr>()).with_graceful_shutdown(shutdown).await
}

/// A path safe to log: only our own fixed routes, never client-chosen text.
fn loggable_path(path: &str) -> String {
    if path == "/healthz" {
        return path.into();
    }
    match path.strip_prefix("/v1/plaid/") {
        Some(rest) if ALLOWED.contains(&rest) => path.into(),
        _ => "<other>".into(),
    }
}

async fn log_requests(State(st): State<Arc<AppState>>, req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path = loggable_path(req.uri().path());
    let start = Instant::now();
    let resp = next.run(req).await;
    let lic = resp.extensions().get::<LicenseTag>().map_or("-", |t| t.0.as_str()).to_string();
    (st.log)(&format!("{method} {path} {} {}ms lic={lic}", resp.status().as_u16(), start.elapsed().as_millis()));
    resp
}

fn client_ip(st: &AppState, headers: &HeaderMap, peer: SocketAddr) -> String {
    if let Some(name) = &st.cfg.client_ip_header
        && let Some(v) = headers.get(name.as_str()).and_then(|v| v.to_str().ok())
        && let Some(last) = v.rsplit(',').next()
    {
        return last.trim().chars().take(64).collect();
    }
    peer.ip().to_string()
}

async fn plaid_route(
    State(st): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(path): Path<String>,
    body: Bytes,
) -> Response {
    let mut tag = None;
    let mut resp = match handle(&st, &headers, peer, &path, &body, &mut tag).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    };
    if let Some(t) = tag {
        resp.extensions_mut().insert(t);
    }
    resp
}

async fn authenticate(st: &AppState, headers: &HeaderMap, ip: &str) -> Result<License, ApiErr> {
    if st.auth_fail.blocked(ip) {
        return Err(ApiErr::new(StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED", "Too many failed sign-ins. Wait a minute and try again."));
    }
    let key = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|k| !k.is_empty() && k.len() <= MAX_KEY_LEN);
    let Some(key) = key else {
        st.auth_fail.hit(ip);
        return Err(ApiErr::new(StatusCode::UNAUTHORIZED, "LICENSE_MISSING", "No Tallybook license key was sent. Add your key in Settings to use bank sync."));
    };
    let hash = sha256_hex(key);
    match st.verdict_for(key, &hash).await {
        Verdict::Valid => Ok(License { hash }),
        Verdict::Lapsed => Err(ApiErr::new(StatusCode::FORBIDDEN, "LICENSE_ENDED", "Your Tallybook plan has ended. Renew to keep your banks syncing.")),
        Verdict::Rejected => {
            st.auth_fail.hit(ip);
            Err(ApiErr::new(
                StatusCode::UNAUTHORIZED,
                "LICENSE_INVALID",
                "This license key isn't a Tallybook plan that includes bank sync. Check the key in Settings, or upgrade your plan.",
            ))
        }
        Verdict::Unavailable => Err(ApiErr::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "LICENSE_CHECK_UNAVAILABLE",
            "Tallybook couldn't verify your plan right now. Try again in a few minutes.",
        )),
    }
}

fn json_response(status: StatusCode, v: Value) -> Response {
    (status, Json(v)).into_response()
}

/// Swaps a client's handle in `body.access_token` for the real token.
/// `Ok(None)`: no token in the body. A handle that is unknown or belongs to
/// another license is a 404, indistinguishably.
fn translate(st: &AppState, lic: &License, body: &mut Value) -> Result<Option<String>, ApiErr> {
    if body.get("access_token").is_some_and(Value::is_null) {
        body.as_object_mut().map(|o| o.remove("access_token"));
    }
    let Some(v) = body.get("access_token") else { return Ok(None) };
    let handle = v.as_str().ok_or_else(|| ApiErr::new(StatusCode::BAD_REQUEST, "INVALID_REQUEST", "access_token must be a string."))?.to_string();
    let real = st.db.token_for(&handle, &lic.hash).map_err(|_| internal())?;
    let Some(real) = real else {
        return Err(ApiErr::new(StatusCode::NOT_FOUND, "UNKNOWN_CONNECTION", "That bank connection isn't on your plan. Reconnect the bank from Settings."));
    };
    body["access_token"] = json!(real);
    Ok(Some(handle))
}

/// Plaid's answer for an item that is already gone on their side.
pub fn item_gone(v: &Value) -> bool {
    matches!(v.get("error_code").and_then(Value::as_str), Some("ITEM_NOT_FOUND" | "INVALID_ACCESS_TOKEN"))
}

impl AppState {
    /// One call to Plaid with our credentials. Any `client_id` or `secret`
    /// already in `body` is replaced. Returns Plaid's status and JSON as-is.
    pub async fn plaid_post(&self, path: &str, mut body: Value) -> Result<(StatusCode, Value), ApiErr> {
        let obj = body.as_object_mut().ok_or_else(internal)?;
        obj.insert("client_id".into(), json!(self.cfg.plaid_client_id));
        obj.insert("secret".into(), json!(self.cfg.plaid_secret));
        let resp = self.http.post(format!("{}/{path}", self.cfg.plaid_base)).json(&body).send().await.map_err(|_| plaid_unreachable())?;
        let status = resp.status();
        let bytes = resp.bytes().await.map_err(|_| plaid_unreachable())?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| plaid_unreachable())?;
        Ok((status, value))
    }
}

/// Derives the stable per-license id Plaid sees as `client_user_id`.
fn client_user_id(license_hash: &str) -> String {
    format!("tbu-{}", &sha256_hex(&format!("tallybook-connect-user:{license_hash}"))[..32])
}

async fn handle(st: &Arc<AppState>, headers: &HeaderMap, peer: SocketAddr, path: &str, raw: &Bytes, tag: &mut Option<LicenseTag>) -> Result<Response, ApiErr> {
    let ip = client_ip(st, headers, peer);
    let lic = authenticate(st, headers, &ip).await?;
    *tag = Some(LicenseTag(lic.hash[..8].to_string()));
    if !st.limiter.hit(&lic.hash) {
        return Err(ApiErr::new(StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED", "Too many requests. Wait a minute and try again."));
    }
    let path = *ALLOWED.iter().find(|p| **p == path).ok_or_else(|| ApiErr::new(StatusCode::NOT_FOUND, "NOT_FOUND", "Unknown endpoint."))?;

    let mut body: Value = if raw.is_empty() {
        json!({})
    } else {
        serde_json::from_slice(raw).ok().filter(Value::is_object).ok_or_else(|| ApiErr::new(StatusCode::BAD_REQUEST, "INVALID_REQUEST", "The request body must be a JSON object."))?
    };
    // Clients never supply credentials; ours are injected in `plaid_post`.
    if let Some(o) = body.as_object_mut() {
        o.remove("client_id");
        o.remove("secret");
    }
    let handle = translate(st, &lic, &mut body)?;
    let max = st.cfg.max_items_per_license;
    let now = chrono::Utc::now().timestamp();

    match path {
        "link/token/create" => {
            if let Some(o) = body.as_object_mut() {
                for k in ["required_if_supported_products", "optional_products", "additional_consented_products", "webhook"] {
                    o.remove(k);
                }
            }
            if handle.is_none() {
                if st.db.count_items(&lic.hash).map_err(|_| internal())? >= max {
                    return Err(item_limit(max));
                }
                body["products"] = json!(["transactions"]);
            } else if let Some(o) = body.as_object_mut() {
                o.remove("products");
            }
            // Whatever the client claims, Plaid sees an id derived from the license.
            if !body.get("user").is_some_and(Value::is_object) {
                body["user"] = json!({});
            }
            body["user"]["client_user_id"] = json!(client_user_id(&lic.hash));
            let (status, v) = st.plaid_post(path, body).await?;
            Ok(json_response(status, v))
        }
        "item/public_token/exchange" => {
            let (status, mut v) = st.plaid_post(path, body).await?;
            if status.is_success() {
                let (Some(token), Some(item_id)) = (v.get("access_token").and_then(Value::as_str).map(str::to_string), v.get("item_id").and_then(Value::as_str).map(str::to_string)) else {
                    return Err(plaid_unreachable());
                };
                let new_handle = format!("{HANDLE_PREFIX}{}", uuid::Uuid::new_v4());
                match st.db.insert_item_capped(&new_handle, &lic.hash, &item_id, &token, now, max) {
                    Ok(true) => v["access_token"] = json!(new_handle),
                    stored => {
                        // Over the cap, or we couldn't save it: don't keep a
                        // billable Item nobody can reach.
                        let _ = st.plaid_post("item/remove", json!({ "access_token": token })).await;
                        return Err(if stored.is_ok() { item_limit(max) } else { internal() });
                    }
                }
            }
            Ok(json_response(status, v))
        }
        "item/remove" => {
            let (status, v) = st.plaid_post(path, body).await?;
            if let Some(h) = handle
                && (status.is_success() || item_gone(&v))
            {
                st.db.delete_item(&h).map_err(|_| internal())?;
            }
            Ok(json_response(status, v))
        }
        _ => {
            let (status, v) = st.plaid_post(path, body).await?;
            Ok(json_response(status, v))
        }
    }
}

fn item_limit(max: u32) -> ApiErr {
    ApiErr::new(
        StatusCode::FORBIDDEN,
        "ITEM_LIMIT_REACHED",
        format!("Your plan includes up to {max} connected banks and you've reached that limit. Disconnect one to add another."),
    )
}
