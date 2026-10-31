//! Bill reminders on the user's own calendar: Google Calendar or Outlook.
//!
//! The user signs in once in their browser ([`oauth`]); Tallybook then keeps
//! a calendar of its own there ("Tallybook bills") holding one all-day event
//! per upcoming due date. It never reads or touches any other calendar.
//!
//! Each event Tallybook writes carries a hidden tag, `<key>|<fingerprint>`:
//! the key says which due date it stands for, the fingerprint what it last
//! said. A sync lists the tagged events from today on, then adds, rewrites
//! or removes events until they match the book. Nothing about the calendar
//! needs to be kept in the book except which calendar it is, and events the
//! user adds to that calendar by hand (untagged) are left alone.
//!
//! Single events, not repeating series: Tallybook moves a bill due on the
//! 31st to the end of shorter months, which a calendar's own repeat rules
//! would skip, and a due date that's been paid should drop off the calendar.

pub mod google;
pub mod oauth;
pub mod outlook;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The name of the calendar Tallybook makes for itself.
pub const CALENDAR_NAME: &str = "Tallybook bills";

/// How long any one request to a calendar service may take.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Google,
    Outlook,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Google => "google",
            Provider::Outlook => "outlook",
        }
    }

    pub fn parse(s: &str) -> Option<Provider> {
        match s {
            "google" => Some(Provider::Google),
            "outlook" => Some(Provider::Outlook),
            _ => None,
        }
    }
}

/// One due date, as it should appear on the calendar: an all-day event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// Which due date this is, stable across syncs, e.g. `rb:12:2026-11-01`.
    pub key: String,
    pub title: String,
    pub description: String,
    pub date: NaiveDate,
    /// A reminder this many minutes before the day starts; `None` for none.
    pub reminder_minutes: Option<u32>,
}

impl Event {
    /// What the event says, hashed: when it changes, the event is rewritten.
    pub fn fingerprint(&self) -> String {
        let mut h = Sha256::new();
        for part in [self.title.as_str(), self.description.as_str(), &self.date.to_string(), &format!("{:?}", self.reminder_minutes)] {
            h.update(part.as_bytes());
            h.update([0]);
        }
        h.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect()
    }

    /// The hidden tag the event carries on the calendar.
    pub fn tag(&self) -> String {
        format!("{}|{}", self.key, self.fingerprint())
    }
}

/// An event Tallybook wrote earlier, as the calendar lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteEvent {
    /// The calendar service's own id for it.
    pub id: String,
    pub key: String,
    pub fingerprint: String,
}

impl RemoteEvent {
    /// Reads an event's tag; `None` for an event Tallybook didn't write.
    fn from_tag(id: String, tag: &str) -> Option<RemoteEvent> {
        let (key, fingerprint) = tag.rsplit_once('|')?;
        (!key.is_empty()).then(|| RemoteEvent { id, key: key.to_string(), fingerprint: fingerprint.to_string() })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CalendarError {
    #[error("network error talking to the calendar: {0}")]
    Http(#[from] reqwest::Error),
    /// The sign-in was revoked or has expired: sign in again.
    #[error("the calendar sign-in has expired or was removed; connect the calendar again")]
    Auth,
    /// Tallybook's calendar is gone (deleted by hand, say).
    #[error("the Tallybook calendar wasn't found")]
    NotFound,
    #[error("the calendar service said: {0}")]
    Api(String),
    #[error("{0}")]
    SignIn(String),
}

pub type Result<T> = std::result::Result<T, CalendarError>;

/// The changes one sync made.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SyncReport {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
}

/// A signed-in connection to one calendar service.
pub struct Client {
    provider: Provider,
    http: reqwest::Client,
    access_token: String,
    /// The API's address; tests point it at a local server.
    base: String,
}

impl Client {
    pub fn new(provider: Provider, access_token: String) -> Client {
        let base = match provider {
            Provider::Google => google::API_BASE,
            Provider::Outlook => outlook::API_BASE,
        };
        Client::with_base(provider, access_token, base)
    }

    pub fn with_base(provider: Provider, access_token: String, base: &str) -> Client {
        Client { provider, http: http_client(), access_token, base: base.trim_end_matches('/').to_string() }
    }

    /// Makes Tallybook's calendar and returns its id.
    pub async fn create_calendar(&self) -> Result<String> {
        match self.provider {
            Provider::Google => google::create_calendar(self).await,
            Provider::Outlook => outlook::create_calendar(self).await,
        }
    }

    /// Removes Tallybook's calendar and every event on it. Already gone is fine.
    pub async fn delete_calendar(&self, calendar_id: &str) -> Result<()> {
        let r = match self.provider {
            Provider::Google => google::delete_calendar(self, calendar_id).await,
            Provider::Outlook => outlook::delete_calendar(self, calendar_id).await,
        };
        match r {
            Err(CalendarError::NotFound) => Ok(()),
            r => r,
        }
    }

    /// The events Tallybook wrote, on days from `from` on.
    pub async fn list(&self, calendar_id: &str, from: NaiveDate) -> Result<Vec<RemoteEvent>> {
        match self.provider {
            Provider::Google => google::list(self, calendar_id, from).await,
            Provider::Outlook => outlook::list(self, calendar_id, from).await,
        }
    }

    pub async fn create(&self, calendar_id: &str, e: &Event) -> Result<()> {
        match self.provider {
            Provider::Google => google::create(self, calendar_id, e).await,
            Provider::Outlook => outlook::create(self, calendar_id, e).await,
        }
    }

    pub async fn update(&self, calendar_id: &str, remote_id: &str, e: &Event) -> Result<()> {
        match self.provider {
            Provider::Google => google::update(self, calendar_id, remote_id, e).await,
            Provider::Outlook => outlook::update(self, calendar_id, remote_id, e).await,
        }
    }

    /// Removes one event. Already gone is fine.
    pub async fn delete(&self, calendar_id: &str, remote_id: &str) -> Result<()> {
        let r = match self.provider {
            Provider::Google => google::delete(self, calendar_id, remote_id).await,
            Provider::Outlook => outlook::delete(self, calendar_id, remote_id).await,
        };
        match r {
            Err(CalendarError::NotFound) => Ok(()),
            r => r,
        }
    }

    /// Makes the calendar's events from `today` on match `want`: adds what's
    /// missing, rewrites what changed, removes what's no longer due. Events
    /// before `today` are history and stay as they are.
    pub async fn sync(&self, calendar_id: &str, today: NaiveDate, want: &[Event]) -> Result<SyncReport> {
        let plan = plan(self.list(calendar_id, today).await?, want);
        for e in &plan.create {
            self.create(calendar_id, e).await?;
        }
        for (id, e) in &plan.update {
            self.update(calendar_id, id, e).await?;
        }
        for id in &plan.delete {
            self.delete(calendar_id, id).await?;
        }
        Ok(SyncReport { added: plan.create.len(), updated: plan.update.len(), removed: plan.delete.len() })
    }

    fn url(&self, segments: &[&str]) -> url::Url {
        let mut u = url::Url::parse(&self.base).expect("calendar API base is a valid URL");
        u.path_segments_mut().expect("calendar API base has a path").extend(segments);
        u
    }

    /// Sends a request and reads the reply as JSON (`Value::Null` when empty).
    async fn send(&self, req: reqwest::RequestBuilder) -> Result<serde_json::Value> {
        let resp = req.bearer_auth(&self.access_token).timeout(REQUEST_TIMEOUT).send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        match status.as_u16() {
            200..=299 if text.trim().is_empty() => Ok(serde_json::Value::Null),
            200..=299 => serde_json::from_str(&text).map_err(|e| CalendarError::Api(format!("unreadable reply: {e}"))),
            401 => Err(CalendarError::Auth),
            404 | 410 => Err(CalendarError::NotFound),
            _ => Err(CalendarError::Api(api_message(status, &text))),
        }
    }
}

/// What one sync will do.
#[derive(Debug, Default, PartialEq, Eq)]
struct Plan<'a> {
    create: Vec<&'a Event>,
    /// The remote id, and what it should now say.
    update: Vec<(String, &'a Event)>,
    delete: Vec<String>,
}

fn plan(have: Vec<RemoteEvent>, want: &[Event]) -> Plan<'_> {
    let mut by_key: std::collections::HashMap<String, RemoteEvent> = std::collections::HashMap::new();
    let mut out = Plan::default();
    for r in have {
        // Two events with one key (a sync cut off halfway, say): keep one.
        if let Some(old) = by_key.insert(r.key.clone(), r) {
            out.delete.push(old.id);
        }
    }
    for e in want {
        match by_key.remove(&e.key) {
            None => out.create.push(e),
            Some(r) if r.fingerprint != e.fingerprint() => out.update.push((r.id, e)),
            Some(_) => {}
        }
    }
    let mut gone: Vec<String> = by_key.into_values().map(|r| r.id).collect();
    gone.sort();
    out.delete.extend(gone);
    out
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder().timeout(REQUEST_TIMEOUT).build().unwrap_or_default()
}

/// The service's error message when it sent one, else the status. Kept
/// short: it may be shown to the user.
fn api_message(status: reqwest::StatusCode, body: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let msg = v["error"]["message"].as_str().or_else(|| v["error_description"].as_str()).or_else(|| v["error"].as_str()).unwrap_or("");
    let msg: String = msg.chars().take(200).collect();
    if msg.is_empty() { format!("HTTP {status}") } else { format!("{msg} (HTTP {})", status.as_u16()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> Event {
        Event {
            key: "rb:3:2026-11-01".into(),
            title: "Due: Rent".into(),
            description: String::new(),
            date: NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
            reminder_minutes: Some(900),
        }
    }

    #[test]
    fn the_fingerprint_follows_what_the_event_says() {
        let a = event();
        assert_eq!(a.fingerprint(), event().fingerprint());
        assert_eq!(a.fingerprint().len(), 16);
        let b = Event { title: "Due: Rent — $1,450.00".into(), ..event() };
        let c = Event { reminder_minutes: None, ..event() };
        assert_ne!(a.fingerprint(), b.fingerprint());
        assert_ne!(a.fingerprint(), c.fingerprint());
    }

    #[test]
    fn a_tag_reads_back_and_strangers_are_ignored() {
        let e = event();
        let r = RemoteEvent::from_tag("x".into(), &e.tag()).unwrap();
        assert_eq!((r.key.as_str(), r.fingerprint), ("rb:3:2026-11-01", e.fingerprint()));
        assert!(RemoteEvent::from_tag("x".into(), "no bar").is_none());
        assert!(RemoteEvent::from_tag("x".into(), "|abc").is_none());
    }

    #[test]
    fn a_sync_adds_rewrites_and_removes_only_what_changed() {
        let d = |day| NaiveDate::from_ymd_opt(2026, 11, day).unwrap();
        let ev = |key: &str, title: &str, day| Event { key: key.into(), title: title.into(), description: String::new(), date: d(day), reminder_minutes: None };
        let remote = |id: &str, e: &Event| RemoteEvent { id: id.into(), key: e.key.clone(), fingerprint: e.fingerprint() };
        let same = ev("rb:1:2026-11-01", "Due: Rent", 1);
        let renamed_old = ev("rb:2:2026-11-05", "Due: Power", 5);
        let renamed = ev("rb:2:2026-11-05", "Due: Electric", 5);
        let paid = ev("bill:3", "Due: Acme bill #3", 9);
        let new = ev("bill:4", "Due: Acme bill #4", 20);
        let have = vec![remote("r1", &same), remote("r2", &renamed_old), remote("r3", &paid), remote("r1-dupe", &same)];
        let want = [same.clone(), renamed.clone(), new.clone()];
        let p = plan(have, &want);
        assert_eq!(p.create, vec![&new]);
        assert_eq!(p.update, vec![("r2".to_string(), &renamed)]);
        assert_eq!(p.delete, vec!["r1".to_string(), "r3".to_string()]);
        // Run again against what that left: nothing to do.
        let after = vec![remote("r1-dupe", &same), remote("r2", &renamed), remote("r4", &new)];
        assert_eq!(plan(after, &want), Plan::default());
    }

    #[test]
    fn urls_escape_calendar_ids() {
        let c = Client::with_base(Provider::Google, "t".into(), "https://example.test/calendar/v3/");
        let u = c.url(&["calendars", "abc@group.calendar.google.com", "events"]);
        assert_eq!(u.as_str(), "https://example.test/calendar/v3/calendars/abc@group.calendar.google.com/events");
        let u = c.url(&["calendars", "a/b?c"]);
        assert_eq!(u.as_str(), "https://example.test/calendar/v3/calendars/a%2Fb%3Fc");
    }
}
