//! Outlook and Microsoft 365 calendars, through Microsoft Graph (v1.0).
//!
//! Graph has no narrower permission than `Calendars.ReadWrite`, but Tallybook
//! only ever touches the calendar it made. Its tag rides in a single-value
//! extended property under a name of its own.

use crate::{CALENDAR_NAME, CalendarError, Client, Event, RemoteEvent, Result};
use chrono::{Days, NaiveDate};
use serde_json::{Value, json};

pub const API_BASE: &str = "https://graph.microsoft.com/v1.0";
pub const AUTH_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";
pub const TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";
pub const SCOPE: &str = "offline_access Calendars.ReadWrite";

/// The extended property holding Tallybook's tag. The GUID is Tallybook's
/// own property set; never change it, or old events stop being recognized.
const TAG: &str = "String {8d3c4a52-6f0b-4b7e-9a51-3c2e7f1d9b40} Name tallybook";
const PAGE_SIZE: &str = "200";

pub(crate) async fn create_calendar(c: &Client) -> Result<String> {
    let v = c.send(c.http.post(c.url(&["me", "calendars"])).json(&json!({ "name": CALENDAR_NAME }))).await?;
    v["id"].as_str().map(str::to_string).ok_or_else(|| CalendarError::Api("no calendar id in the reply".into()))
}

pub(crate) async fn delete_calendar(c: &Client, calendar_id: &str) -> Result<()> {
    c.send(c.http.delete(c.url(&["me", "calendars", calendar_id]))).await.map(drop)
}

pub(crate) async fn list(c: &Client, calendar_id: &str, from: NaiveDate) -> Result<Vec<RemoteEvent>> {
    let mut u = c.url(&["me", "calendars", calendar_id, "events"]);
    u.query_pairs_mut()
        .append_pair("$filter", &format!("start/dateTime ge '{from}T00:00:00'"))
        .append_pair("$select", "id")
        .append_pair("$expand", &format!("singleValueExtendedProperties($filter=id eq '{TAG}')"))
        .append_pair("$top", PAGE_SIZE);
    let mut out = Vec::new();
    let mut next = Some(u);
    while let Some(u) = next.take() {
        let v = c.send(c.http.get(u)).await?;
        out.extend(parse_list(&v));
        // Follow the next page only on the same service: it gets the sign-in.
        next = v["@odata.nextLink"].as_str().filter(|l| l.starts_with(&c.base)).and_then(|l| url::Url::parse(l).ok());
    }
    Ok(out)
}

/// The tagged events in one page of the events list.
fn parse_list(v: &Value) -> Vec<RemoteEvent> {
    v["value"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let tag = e["singleValueExtendedProperties"].as_array()?.iter().find(|p| p["id"].as_str().is_some_and(|id| id.eq_ignore_ascii_case(TAG)))?;
            RemoteEvent::from_tag(e["id"].as_str()?.to_string(), tag["value"].as_str()?)
        })
        .collect()
}

pub(crate) async fn create(c: &Client, calendar_id: &str, e: &Event) -> Result<()> {
    c.send(c.http.post(c.url(&["me", "calendars", calendar_id, "events"])).json(&body(e))).await.map(drop)
}

pub(crate) async fn update(c: &Client, _calendar_id: &str, remote_id: &str, e: &Event) -> Result<()> {
    c.send(c.http.patch(c.url(&["me", "events", remote_id])).json(&body(e))).await.map(drop)
}

pub(crate) async fn delete(c: &Client, _calendar_id: &str, remote_id: &str) -> Result<()> {
    c.send(c.http.delete(c.url(&["me", "events", remote_id]))).await.map(drop)
}

/// An all-day event shown as free. Graph wants all-day events to run
/// midnight to midnight in a stated time zone; the date is what shows.
fn body(e: &Event) -> Value {
    let day = |d: NaiveDate| json!({ "dateTime": format!("{d}T00:00:00"), "timeZone": "UTC" });
    json!({
        "subject": e.title,
        "body": { "contentType": "text", "content": e.description },
        "start": day(e.date),
        "end": day(e.date + Days::new(1)),
        "isAllDay": true,
        "showAs": "free",
        "isReminderOn": e.reminder_minutes.is_some(),
        "reminderMinutesBeforeStart": e.reminder_minutes.unwrap_or(0),
        "singleValueExtendedProperties": [{ "id": TAG, "value": e.tag() }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_event_is_all_day_with_its_reminder_and_tag() {
        let e = Event {
            key: "rb:4:2026-11-30".into(),
            title: "Due: Power".into(),
            description: String::new(),
            date: NaiveDate::from_ymd_opt(2026, 11, 30).unwrap(),
            reminder_minutes: None,
        };
        let b = body(&e);
        assert_eq!(b["start"]["dateTime"], "2026-11-30T00:00:00");
        assert_eq!(b["end"]["dateTime"], "2026-12-01T00:00:00");
        assert_eq!(b["isAllDay"], true);
        assert_eq!(b["isReminderOn"], false);
        assert_eq!(b["singleValueExtendedProperties"][0]["value"], e.tag());
    }

    #[test]
    fn listing_keeps_only_tagged_events() {
        let v = json!({ "value": [
            { "id": "a", "singleValueExtendedProperties": [{ "id": TAG.to_lowercase(), "value": "bill:9|ab12" }] },
            { "id": "b", "singleValueExtendedProperties": [] },
            { "id": "c" },
        ]});
        assert_eq!(parse_list(&v), vec![RemoteEvent { id: "a".into(), key: "bill:9".into(), fingerprint: "ab12".into() }]);
    }
}
