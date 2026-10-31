//! Signing in to a calendar service from a desktop app: OAuth 2.0 with PKCE
//! and a loopback redirect (RFC 8252).
//!
//! [`start`] listens on a free port on 127.0.0.1 and returns the sign-in
//! page to open in the browser. When the user approves, the service sends
//! the browser back to that port with a one-time code, which
//! [`Pending::wait_for_code`] takes and [`exchange`] trades for tokens.
//! The refresh token is what Tallybook keeps (in the OS keychain); each sync
//! trades it for a short-lived access token with [`refresh`].
//!
//! A desktop app can't keep a secret, so PKCE does that job: only this
//! process knows the verifier the code is bound to. Google still issues
//! "desktop" clients a `client_secret`, which it documents as not secret.

use crate::{CalendarError, Provider, Result, google, http_client, outlook};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// The longest a request to the loopback listener may be, and take.
const MAX_REQUEST: usize = 8 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// Tallybook's registration with one calendar service.
#[derive(Debug, Clone)]
pub struct AppId {
    pub provider: Provider,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub auth_url: String,
    pub token_url: String,
}

impl AppId {
    pub fn new(provider: Provider, client_id: String, client_secret: Option<String>) -> AppId {
        let (auth, token) = match provider {
            Provider::Google => (google::AUTH_URL, google::TOKEN_URL),
            Provider::Outlook => (outlook::AUTH_URL, outlook::TOKEN_URL),
        };
        AppId { provider, client_id, client_secret, auth_url: auth.into(), token_url: token.into() }
    }

    fn scope(&self) -> &'static str {
        match self.provider {
            Provider::Google => google::SCOPE,
            Provider::Outlook => outlook::SCOPE,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Tokens {
    pub access_token: String,
    /// Sent on first sign-in; on a refresh, only when the service rotates it.
    pub refresh_token: Option<String>,
}

/// A sign-in waiting for the browser to come back.
pub struct Pending {
    listener: TcpListener,
    verifier: String,
    state: String,
    redirect_uri: String,
}

/// Begins a sign-in: returns it, and the page to open in the browser.
pub async fn start(app: &AppId) -> std::io::Result<(Pending, String)> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let redirect_uri = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
    let verifier = random_token();
    let state = random_token();
    let mut u = url::Url::parse(&app.auth_url).map_err(std::io::Error::other)?;
    {
        let mut q = u.query_pairs_mut();
        q.append_pair("client_id", &app.client_id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("scope", app.scope())
            .append_pair("code_challenge", &challenge(&verifier))
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state);
        match app.provider {
            // Without these Google sends no refresh token on a second sign-in.
            Provider::Google => q.append_pair("access_type", "offline").append_pair("prompt", "consent"),
            Provider::Outlook => q.append_pair("response_mode", "query").append_pair("prompt", "select_account"),
        };
    }
    Ok((Pending { listener, verifier, state, redirect_uri }, u.into()))
}

impl Pending {
    /// Waits for the browser to come back with the code. Requests that
    /// aren't the answer (a favicon, a stale or forged `state`) are turned
    /// away and the wait goes on; a refusal ends it.
    pub async fn wait_for_code(&self) -> Result<String> {
        loop {
            let (mut sock, _) = self.listener.accept().await.map_err(|e| CalendarError::SignIn(format!("sign-in listener failed: {e}")))?;
            let Ok(Some(target)) = tokio::time::timeout(READ_TIMEOUT, read_target(&mut sock)).await.unwrap_or(Ok(None)) else {
                continue;
            };
            match callback(&target, &self.state) {
                Callback::Code(code) => {
                    reply(&mut sock, "200 OK", "Tallybook is connected to your calendar. You can close this tab and go back to Tallybook.").await;
                    return Ok(code);
                }
                Callback::Refused(why) => {
                    reply(&mut sock, "200 OK", "The calendar wasn't connected. You can close this tab and go back to Tallybook.").await;
                    return Err(CalendarError::SignIn(why));
                }
                Callback::Ignore => reply(&mut sock, "404 Not Found", "Not found.").await,
            }
        }
    }
}

/// Trades the code from the browser for tokens.
pub async fn exchange(app: &AppId, pending: &Pending, code: &str) -> Result<Tokens> {
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("client_id", &app.client_id),
        ("redirect_uri", &pending.redirect_uri),
        ("code_verifier", &pending.verifier),
    ];
    if let Some(s) = &app.client_secret {
        form.push(("client_secret", s));
    }
    let t = token_request(app, &form).await?;
    if t.refresh_token.is_none() {
        return Err(CalendarError::SignIn("the calendar service didn't grant lasting access; try connecting again".into()));
    }
    Ok(t)
}

/// A fresh access token from the saved refresh token.
pub async fn refresh(app: &AppId, refresh_token: &str) -> Result<Tokens> {
    let mut form = vec![("grant_type", "refresh_token"), ("refresh_token", refresh_token), ("client_id", &app.client_id)];
    if let Some(s) = &app.client_secret {
        form.push(("client_secret", s));
    }
    if app.provider == Provider::Outlook {
        form.push(("scope", outlook::SCOPE));
    }
    token_request(app, &form).await
}

/// Tells Google to forget the sign-in. Microsoft has no such call for a
/// token; the user removes the app from their account page instead.
pub async fn revoke(app: &AppId, refresh_token: &str) -> Result<()> {
    if app.provider == Provider::Google {
        let resp = http_client().post(google::REVOKE_URL).form(&[("token", refresh_token)]).send().await?;
        if !resp.status().is_success() && resp.status().as_u16() != 400 {
            return Err(CalendarError::Api(format!("revoking the sign-in failed: HTTP {}", resp.status())));
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct TokenReply {
    access_token: Option<String>,
    refresh_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

async fn token_request(app: &AppId, form: &[(&str, &str)]) -> Result<Tokens> {
    let resp = http_client().post(&app.token_url).header("Accept", "application/json").form(form).send().await?;
    let status = resp.status();
    let r: TokenReply = resp.json().await.map_err(|e| CalendarError::Api(format!("unreadable sign-in reply (HTTP {status}): {e}")))?;
    match (r.access_token, r.error.as_deref()) {
        (Some(access_token), None) => Ok(Tokens { access_token, refresh_token: r.refresh_token }),
        // The refresh token was revoked, expired, or the password changed.
        (_, Some("invalid_grant")) => Err(CalendarError::Auth),
        (_, e) => {
            let msg: String = r.error_description.as_deref().or(e).unwrap_or("no access token in the reply").chars().take(200).collect();
            Err(CalendarError::SignIn(format!("sign-in failed: {msg}")))
        }
    }
}

/// 256 random bits, base64url: a PKCE verifier (43 chars, as RFC 7636 asks) or a `state`.
fn random_token() -> String {
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    URL_SAFE_NO_PAD.encode(bytes)
}

fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

#[derive(Debug, PartialEq, Eq)]
enum Callback {
    Code(String),
    Refused(String),
    Ignore,
}

/// What a request to the loopback listener says. Only the root path with
/// this sign-in's `state` counts.
fn callback(target: &str, state: &str) -> Callback {
    let Ok(u) = url::Url::parse(&format!("http://127.0.0.1{target}")) else { return Callback::Ignore };
    if u.path() != "/" {
        return Callback::Ignore;
    }
    let q: std::collections::HashMap<String, String> = u.query_pairs().into_owned().collect();
    if q.get("state").map(String::as_str) != Some(state) {
        return Callback::Ignore;
    }
    match (q.get("code"), q.get("error")) {
        (Some(code), None) if !code.is_empty() => Callback::Code(code.clone()),
        (_, Some(e)) if e == "access_denied" => Callback::Refused("the sign-in was cancelled".into()),
        (_, e) => Callback::Refused(format!("the sign-in didn't finish ({})", e.map(String::as_str).unwrap_or("no code"))),
    }
}

/// The target of a `GET` request (`/?code=…`), or `None` for anything else.
async fn read_target(sock: &mut tokio::net::TcpStream) -> std::io::Result<Option<String>> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    while !buf.windows(2).any(|w| w == b"\r\n") {
        let n = sock.read(&mut chunk).await?;
        if n == 0 || buf.len() + n > MAX_REQUEST {
            return Ok(None);
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let line = String::from_utf8_lossy(&buf);
    let mut parts = line.lines().next().unwrap_or("").split(' ');
    Ok(match (parts.next(), parts.next()) {
        (Some("GET"), Some(t)) if t.starts_with('/') => Some(t.to_string()),
        _ => None,
    })
}

async fn reply(sock: &mut tokio::net::TcpStream, status: &str, message: &str) {
    let body = format!(
        "<!doctype html><meta charset=utf-8><title>Tallybook</title>\
         <body style=\"font:16px system-ui;margin:4em auto;max-width:32em;text-align:center\"><p>{message}</p>"
    );
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = sock.write_all(resp.as_bytes()).await;
    let _ = sock.shutdown().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_challenge_is_rfc_7636s() {
        // RFC 7636, appendix B.
        assert_eq!(challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        let v = random_token();
        assert_eq!(v.len(), 43);
        assert_ne!(v, random_token());
    }

    #[test]
    fn only_the_answer_with_our_state_counts() {
        assert_eq!(callback("/?code=abc&state=s1", "s1"), Callback::Code("abc".into()));
        assert_eq!(callback("/?code=abc&state=forged", "s1"), Callback::Ignore);
        assert_eq!(callback("/?code=abc", "s1"), Callback::Ignore);
        assert_eq!(callback("/favicon.ico", "s1"), Callback::Ignore);
        assert_eq!(callback("/other?code=abc&state=s1", "s1"), Callback::Ignore);
        assert_eq!(callback("/?error=access_denied&state=s1", "s1"), Callback::Refused("the sign-in was cancelled".into()));
        assert!(matches!(callback("/?error=server_error&state=s1", "s1"), Callback::Refused(_)));
    }

    #[tokio::test]
    async fn the_sign_in_page_asks_for_the_right_things() {
        let app = AppId::new(Provider::Google, "cid".into(), Some("sec".into()));
        let (p, url) = start(&app).await.unwrap();
        let u = url::Url::parse(&url).unwrap();
        let q: std::collections::HashMap<String, String> = u.query_pairs().into_owned().collect();
        assert_eq!(u.host_str(), Some("accounts.google.com"));
        assert_eq!(q["client_id"], "cid");
        assert_eq!(q["scope"], google::SCOPE);
        assert_eq!(q["code_challenge"], challenge(&p.verifier));
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["state"], p.state);
        assert_eq!(q["redirect_uri"], p.redirect_uri);
        assert!(p.redirect_uri.starts_with("http://127.0.0.1:"));
        assert!(!q.contains_key("client_secret"), "the secret never goes to the browser");
    }

    #[tokio::test]
    async fn the_listener_waits_past_strays_for_the_code() {
        let app = AppId::new(Provider::Outlook, "cid".into(), None);
        let (p, _) = start(&app).await.unwrap();
        let addr = p.listener.local_addr().unwrap();
        let state = p.state.clone();
        let browser = tokio::spawn(async move {
            let mut out = Vec::new();
            for target in ["/favicon.ico".to_string(), "/?code=bad&state=forged".into(), format!("/?code=the-code&state={state}")] {
                let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
                s.write_all(format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes()).await.unwrap();
                let mut resp = String::new();
                s.read_to_string(&mut resp).await.unwrap();
                out.push(resp.lines().next().unwrap_or("").to_string());
            }
            out
        });
        assert_eq!(p.wait_for_code().await.unwrap(), "the-code");
        let statuses = browser.await.unwrap();
        assert_eq!(statuses, ["HTTP/1.1 404 Not Found", "HTTP/1.1 404 Not Found", "HTTP/1.1 200 OK"]);
    }
}
