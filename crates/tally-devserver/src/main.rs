//! `cargo run -p tally-devserver` → http://127.0.0.1:1441/api/<method>
//!
//! Development only. Binds to localhost, and Vite (port 1440) proxies `/api`
//! here, so the browser and the desktop app exercise the exact same backend.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::Value;
use std::sync::Arc;
use tally_api::{App, Keychain};

async fn call(State(app): State<Arc<App>>, Path(method): Path<String>, body: Option<Json<Value>>) -> Response {
    let params = body.map(|Json(v)| v).unwrap_or(Value::Null);
    match app.call(&method, params).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => (StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({ "code": e.code, "message": e.message }))).into_response(),
    }
}

#[tokio::main]
async fn main() {
    let config = tally_api::default_dir().join(".devserver");
    // Separate keychain service so dev never touches the real app's secrets.
    let app = Arc::new(App::new(Arc::new(Keychain::new("app.tallybook.dev")), config));
    let router = Router::new().route("/api/{method}", post(call)).with_state(app);
    let addr = "127.0.0.1:1441";
    let listener = tokio::net::TcpListener::bind(addr).await.expect("port 1441 is free");
    println!("tally-devserver listening on http://{addr}");
    axum::serve(listener, router).await.expect("server runs");
}
