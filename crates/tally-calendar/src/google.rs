//! Google Calendar (API v3).
//!
//! Tallybook asks only for `calendar.app.created`: it may make calendars and
//! change the events on those, and can't see or touch the user's others.
//! Its tag rides in each event's private extended properties, which only
//! this app's sign-in can read.

use crate::{CALENDAR_NAME, CalendarError, Client, Event, RemoteEvent, Result};
use chrono::{Days, NaiveDate};
use serde_json::{Value, json};

pub const API_BASE: &str = "https://www.googleapis.com/calendar/v3";
pub const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";
pub const SCOPE: &str = "https://www.googleapis.com/auth/calendar.app.created";

/// The private extended property holding Tallybook's tag.
const TAG: &str = "tallybook";
/// Events per page: Google's maximum.
const PAGE_SIZE: &str = "2500";

pub(crate) async fn create_calendar(c: &Client) -> Result<String> {
    let v = c.send(c.http.post(c.url(&["calendars"])).json(&json!({ "summary": CALENDAR_NAME }))).await?;
    v["id"].as_str().map(str::to_string).ok_or_else(|| CalendarError::Api("no calendar id in the reply".into()))
}

pub(crate) async fn delete_calendar(c: &Client, calendar_id: &str) -> Result<()> {
    c.send(c.http.delete(c.url(&["calendars", calendar_id]))).await.map(drop)
}

pub(crate) async fn list(c: &Client, calendar_id: &str, from: NaiveDate) -> Result<Vec<RemoteEvent>> {
    let mut out = Vec::new();
    let mut page: Option<String> = None;
    loop {
        let mut u = c.url(&["calendars", calendar_id, "events"]);
        {
            let mut q = u.query_pairs_mut();
            // timeMin compares with an event's end, so today's all-day events are in.
            q.append_pair("timeMin", &format!("{from}T00:00:00Z"));
            q.append_pair("singleEvents", "true");
            q.append_pair("maxResults", PAGE_SIZE);
            q.append_pair("fields", "items(id,status,extendedProperties),nextPageToken");
            if let Some(p) = &page {
                q.append_pair("pageToken", p);
            }
        }
        let v = c.send(c.http.get(u)).await?;
        out.extend(parse_list(&v));
        match v["nextPageToken"].as_str() {
            Some(p) if !p.is_empty() => page = Some(p.to_string()),
            _ => return Ok(out),
        }
    }
}

/// The tagged events in one page of `events.list`.
fn parse_list(v: &Value) -> Vec<RemoteEvent> {
    v["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| e["status"].as_str() != Some("cancelled"))
        .filter_map(|e| RemoteEvent::from_tag(e["id"].as_str()?.to_string(), e["extendedProperties"]["private"][TAG].as_str()?))
        .collect()
}

pub(crate) async fn create(c: &Client, calendar_id: &str, e: &Event) -> Result<()> {
    c.send(c.http.post(c.url(&["calendars", calendar_id, "events"])).json(&body(e))).await.map(drop)
}

pub(crate) async fn update(c: &Client, calendar_id: &str, remote_id: &str, e: &Event) -> Result<()> {
    c.send(c.http.put(c.url(&["calendars", calendar_id, "events", remote_id])).json(&body(e))).await.map(drop)
}

pub(crate) async fn delete(c: &Client, calendar_id: &str, remote_id: &str) -> Result<()> {
    c.send(c.http.delete(c.url(&["calendars", calendar_id, "events", remote_id]))).await.map(drop)
}

/// An all-day event that doesn't mark the day busy.
fn body(e: &Event) -> Value {
    let overrides: Vec<Value> = e.reminder_minutes.iter().map(|m| json!({ "method": "popup", "minutes": m })).collect();
    json!({
        "summary": e.title,
        "description": e.description,
        "start": { "date": e.date.to_string() },
        // All-day events end the day after (exclusive).
        "end": { "date": (e.date + Days::new(1)).to_string() },
        "transparency": "transparent",
        "reminders": { "useDefault": false, "overrides": overrides },
        "extendedProperties": { "private": { TAG: e.tag() } },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_event_is_all_day_with_its_reminder_and_tag() {
        let e = Event {
            key: "bill:7".into(),
            title: "Due: Acme bill #12".into(),
            description: "Added by Tallybook.".into(),
            date: NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(),
            reminder_minutes: Some(900),
        };
        let b = body(&e);
        assert_eq!(b["start"]["date"], "2026-12-31");
        assert_eq!(b["end"]["date"], "2027-01-01");
        assert_eq!(b["reminders"]["overrides"][0]["minutes"], 900);
        assert_eq!(b["reminders"]["useDefault"], false);
        assert_eq!(b["extendedProperties"]["private"]["tallybook"], e.tag());
        let none = body(&Event { reminder_minutes: None, ..e });
        assert_eq!(none["reminders"]["overrides"], json!([]));
    }

    #[test]
    fn listing_keeps_only_live_tagged_events() {
        let v = json!({ "items": [
            { "id": "a", "status": "confirmed", "extendedProperties": { "private": { "tallybook": "rb:1:2026-11-01|00ff" } } },
            { "id": "b", "status": "cancelled", "extendedProperties": { "private": { "tallybook": "rb:2:2026-11-01|00ff" } } },
            { "id": "c", "status": "confirmed" },
            { "id": "d", "extendedProperties": { "private": { "other": "x" } } },
        ]});
        let got = parse_list(&v);
        assert_eq!(got, vec![RemoteEvent { id: "a".into(), key: "rb:1:2026-11-01".into(), fingerprint: "00ff".into() }]);
    }
}
