//! `cargo run -p tally-devserver` → http://127.0.0.1:1441/api/<method>
//!
//! Development only. Binds to localhost, and Vite (port 1440) proxies `/api`
//! here, so the browser and the desktop app exercise the exact same backend.
//!
//! It answers only requests addressed to `127.0.0.1:1441` or `localhost:1441`
//! (so a web page that rebinds its own domain to this machine is refused),
//! from no origin or the Vite dev origin, with a JSON content type. Books
//! live in `~/Documents/Tallybook Dev` unless `TALLYBOOK_DIR` says otherwise,
//! never in the real `~/Documents/Tallybook`.

use axum::extract::{Path, Request, State};
use axum::http::header::{CONTENT_TYPE, HOST, ORIGIN};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::Value;
use std::sync::Arc;
use tally_api::{App, Keychain};

const ADDR: &str = "127.0.0.1:1441";
/// The `Host` a request must carry: this server, by IP or by name.
const HOSTS: [&str; 2] = ["127.0.0.1:1441", "localhost:1441"];
/// The only page origins allowed to call: the Vite dev server.
const ORIGINS: [&str; 2] = ["http://localhost:1440", "http://127.0.0.1:1440"];

async fn call(State(app): State<Arc<App>>, Path(method): Path<String>, body: Option<Json<Value>>) -> Response {
    let params = body.map(|Json(v)| v).unwrap_or(Value::Null);
    match app.call(&method, params).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => (StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({ "code": e.code, "message": e.message }))).into_response(),
    }
}

/// Whether a request may reach the API, judged by its headers.
fn allowed(headers: &HeaderMap) -> Result<(), StatusCode> {
    let header = |name| headers.get(name).and_then(|v| v.to_str().ok());
    if !header(HOST).is_some_and(|h| HOSTS.contains(&h)) {
        return Err(StatusCode::FORBIDDEN);
    }
    if headers.contains_key(ORIGIN) && !header(ORIGIN).is_some_and(|o| ORIGINS.contains(&o)) {
        return Err(StatusCode::FORBIDDEN);
    }
    let json = header(CONTENT_TYPE).is_some_and(|c| c.split(';').next().is_some_and(|m| m.trim().eq_ignore_ascii_case("application/json")));
    if !json {
        return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    Ok(())
}

async fn guard(req: Request, next: Next) -> Response {
    match allowed(req.headers()) {
        Ok(()) => next.run(req).await,
        Err(status) => status.into_response(),
    }
}

fn router(app: Arc<App>) -> Router {
    Router::new().route("/api/{method}", post(call)).layer(middleware::from_fn(guard)).with_state(app)
}

fn main() {
    // Development books stay out of the real books folder. Set before any
    // other thread exists.
    if std::env::var_os("TALLYBOOK_DIR").is_none() {
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
        // SAFETY: single-threaded here; the runtime starts below.
        unsafe { std::env::set_var("TALLYBOOK_DIR", home.join("Documents").join("Tallybook Dev")) };
    }
    tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime").block_on(serve());
}

async fn serve() {
    let config = tally_api::default_dir().join(".devserver");
    // Separate keychain service so dev never touches the real app's secrets.
    let app = Arc::new(App::new(Arc::new(Keychain::new("app.tallybook.dev")), config));
    let listener = tokio::net::TcpListener::bind(ADDR).await.expect("port 1441 is free");
    println!("tally-devserver listening on http://{ADDR} (books in {})", tally_api::default_dir().display());
    axum::serve(listener, router(app)).await.expect("server runs");
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tally_api::MemorySecrets;
    use tower::ServiceExt;

    async fn status(headers: &[(&str, &str)]) -> StatusCode {
        let dir = std::env::temp_dir().join(format!("tally-devserver-test-{}", std::process::id()));
        let app = Arc::new(App::new(Arc::new(MemorySecrets::default()), dir));
        let mut req = Request::post("/api/book.status");
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        router(app).oneshot(req.body(Body::from("null")).unwrap()).await.unwrap().status()
    }

    #[tokio::test]
    async fn only_local_json_requests_from_the_dev_origin_get_through() {
        let json = ("content-type", "application/json");
        assert_eq!(status(&[("host", "127.0.0.1:1441"), ("origin", "http://localhost:1440"), json]).await, StatusCode::OK);
        assert_eq!(status(&[("host", "localhost:1441"), json]).await, StatusCode::OK, "no origin: a script or test");
        assert_eq!(status(&[("host", "evil.example"), json]).await, StatusCode::FORBIDDEN);
        assert_eq!(status(&[("host", "evil.example:1441"), json]).await, StatusCode::FORBIDDEN);
        assert_eq!(status(&[json]).await, StatusCode::FORBIDDEN, "no host");
        assert_eq!(status(&[("host", "127.0.0.1:1441"), ("origin", "https://evil.example"), json]).await, StatusCode::FORBIDDEN);
        assert_eq!(status(&[("host", "127.0.0.1:1441"), ("origin", "null"), json]).await, StatusCode::FORBIDDEN);
        assert_eq!(status(&[("host", "127.0.0.1:1441")]).await, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(status(&[("host", "127.0.0.1:1441"), ("content-type", "text/plain")]).await, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
}
