//! Bill reminders on the user's calendar (opt-in): Google Calendar or Outlook.
//!
//! Methods: `calendar.status`, `calendar.connect_start`,
//! `calendar.connect_finish`, `calendar.connect_cancel`, `calendar.set`,
//! `calendar.sync`, `calendar.disconnect`.
//!
//! Connecting: `connect_start` returns a sign-in page for the UI to open in
//! the browser; `connect_finish` waits (up to [`SIGN_IN_WAIT`]) for the
//! browser to come back, makes a "Tallybook bills" calendar and fills it.
//! After that the UI calls `calendar.sync` whenever bills change, and on a
//! timer, and the calendar follows: every household bill due in the next
//! [`HORIZON_DAYS`] days that isn't paid or skipped, and every open business
//! bill due in that time, one all-day event each.
//!
//! The book keeps which provider and calendar (`calendar.provider`,
//! `calendar.id`) and the name of the keychain entry holding the sign-in
//! (`calendar.secret`, `calendar:<uuid>`); the sign-in itself never goes in
//! the book. Network calls never hold the book lock.

use crate::{ApiError, ApiResult, App, ok, params, today, unknown_method};
use chrono::{Days, Local, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::result::Result;
use std::time::Duration;
use tally_calendar::oauth::{self, AppId, Pending};
use tally_calendar::{CalendarError, Client, Event, Provider};
use tally_store::*;

/// Tallybook's OAuth clients, registered with Google (a "Desktop app"
/// client) and Microsoft (a public client with redirect `http://127.0.0.1`).
/// Empty until registered; `TALLYBOOK_GOOGLE_CLIENT_ID`,
/// `TALLYBOOK_GOOGLE_CLIENT_SECRET` and `TALLYBOOK_MICROSOFT_CLIENT_ID`
/// override them, for development. Google documents a desktop client's
/// secret as not secret: PKCE protects the sign-in.
const GOOGLE_CLIENT_ID: &str = "";
const GOOGLE_CLIENT_SECRET: &str = "";
const MICROSOFT_CLIENT_ID: &str = "";

/// How far ahead the calendar shows bills.
pub(crate) const HORIZON_DAYS: u64 = 90;
/// How long `connect_finish` waits for the browser.
const SIGN_IN_WAIT: Duration = Duration::from_secs(10 * 60);
/// Reminders go off at 9 am, this many days before; 0 is no reminder.
const DEFAULT_REMIND_DAYS: u32 = 1;
const MAX_REMIND_DAYS: u32 = 7;
/// A bill's name is cut to this on the calendar.
const MAX_NAME: usize = 80;

const PROVIDER: &str = "calendar.provider";
const CALENDAR_ID: &str = "calendar.id";
const SECRET: &str = "calendar.secret";
const AMOUNTS: &str = "calendar.amounts";
const REMIND_DAYS: &str = "calendar.remind_days";
const LAST: &str = "calendar.last";
const LAST_ERROR: &str = "calendar.last_error";

/// A sign-in in progress: what it's for, and which book asked.
pub(crate) struct PendingSignIn {
    app: AppId,
    pending: Pending,
    book: PathBuf,
}

#[derive(Deserialize)]
struct Start {
    provider: Provider,
}

#[derive(Deserialize)]
struct SetCalendar {
    show_amounts: Option<bool>,
    remind_days: Option<u32>,
}

#[derive(Serialize)]
struct CalendarStatus {
    connected: bool,
    provider: Option<Provider>,
    show_amounts: bool,
    remind_days: u32,
    /// When it last synced (RFC 3339), and what went wrong since, if anything.
    last: Option<String>,
    last_error: Option<String>,
    /// The services this build of Tallybook can connect to.
    available: Vec<Provider>,
    horizon_days: u64,
}

/// How the book wants its events written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Style {
    pub(crate) show_amounts: bool,
    pub(crate) remind_days: u32,
}

/// Everything a sync needs from the book, read under the lock.
struct Connection {
    provider: Provider,
    calendar_id: String,
    secret_key: String,
    events: Vec<Event>,
}

fn cal_err(e: CalendarError) -> ApiError {
    ApiError::new("calendar", e.to_string())
}

/// Tallybook's registration with `provider`, if this build has one.
fn app_id(provider: Provider) -> Option<AppId> {
    let env = |k: &str, built_in: &str| std::env::var(k).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| built_in.to_string());
    let (id, secret) = match provider {
        Provider::Google => (env("TALLYBOOK_GOOGLE_CLIENT_ID", GOOGLE_CLIENT_ID), Some(env("TALLYBOOK_GOOGLE_CLIENT_SECRET", GOOGLE_CLIENT_SECRET))),
        Provider::Outlook => (env("TALLYBOOK_MICROSOFT_CLIENT_ID", MICROSOFT_CLIENT_ID), None),
    };
    (!id.is_empty()).then(|| AppId::new(provider, id, secret.filter(|s| !s.is_empty())))
}

/// The keychain entry named in the book, if it's one Tallybook made
/// (`calendar:<uuid>`). The book may come from someone else, so any other
/// name is treated as no sign-in at all.
pub(crate) fn secret_key(stored: &str) -> Option<&str> {
    let id = stored.strip_prefix("calendar:")?;
    let canonical = uuid::Uuid::parse_str(id).ok()?.hyphenated().to_string();
    (canonical == id).then_some(stored)
}

fn style(s: &Store) -> Result<Style, StoreError> {
    Ok(Style {
        show_amounts: s.pref(AMOUNTS)?.as_deref() == Some("1"),
        remind_days: s.pref(REMIND_DAYS)?.and_then(|d| d.parse().ok()).unwrap_or(DEFAULT_REMIND_DAYS).min(MAX_REMIND_DAYS),
    })
}

/// A name as it goes on the calendar: one line, not too long.
fn clean(name: &str) -> String {
    let one_line: String = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= MAX_NAME { one_line } else { format!("{}…", one_line.chars().take(MAX_NAME - 1).collect::<String>()) }
}

/// The events the calendar should hold, from today through the horizon:
/// household due dates that aren't paid or skipped, and business bills with
/// money still owing. `varies` says which household bills move around.
pub(crate) fn wanted(due: &[DueDate], varies: &HashMap<RecurringBillId, bool>, bills: &[BillSummary], style: Style, today: NaiveDate) -> Vec<Event> {
    let horizon = today + Days::new(HORIZON_DAYS);
    // 9 am, `remind_days` before the day starts.
    let reminder_minutes = (style.remind_days > 0).then(|| style.remind_days * 24 * 60 - 15 * 60);
    let amount = |c: tally_core::Cents, about: bool| {
        if style.show_amounts && c.0 > 0 { format!(" — {}${c}", if about { "about " } else { "" }) } else { String::new() }
    };
    let household = due.iter().filter(|d| d.due_date >= today && d.due_date <= horizon && !matches!(d.state, DueState::Paid | DueState::Skipped)).map(|d| {
        let mut description = String::new();
        if d.auto_pay {
            description.push_str("Paid automatically.\n");
        }
        description.push_str("From Tallybook. It comes off this calendar once it's paid.");
        Event {
            key: format!("rb:{}:{}", d.bill_id, d.due_date),
            title: format!("Due: {}{}", clean(&d.name), amount(d.amount, varies.get(&d.bill_id).copied().unwrap_or(false))),
            description,
            date: d.due_date,
            reminder_minutes,
        }
    });
    let business = bills
        .iter()
        .filter(|b| matches!(b.status, DocStatus::Open | DocStatus::Partial | DocStatus::Overdue) && b.balance.0 > 0)
        .filter(|b| b.due_date >= today && b.due_date <= horizon)
        .map(|b| {
            let number = if b.number.trim().is_empty() { String::new() } else { format!(" (bill {})", clean(&b.number)) };
            Event {
                key: format!("bill:{}", b.id),
                title: format!("Due: {}{number}{}", clean(&b.vendor_name), amount(b.balance, false)),
                description: "From Tallybook. It comes off this calendar once it's paid.".into(),
                date: b.due_date,
                reminder_minutes,
            }
        });
    let mut out: Vec<Event> = household.chain(business).collect();
    out.sort_by(|a, b| (a.date, &a.key).cmp(&(b.date, &b.key)));
    out
}

impl App {
    pub(crate) async fn calendar(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "calendar.status" => self.with_api(|s| {
                let pref = |k: &str| s.pref(k).ok().flatten();
                let st = style(s)?;
                let provider = pref(PROVIDER).as_deref().and_then(Provider::parse);
                Ok(CalendarStatus {
                    connected: provider.is_some() && pref(CALENDAR_ID).is_some() && pref(SECRET).as_deref().and_then(secret_key).is_some(),
                    provider,
                    show_amounts: st.show_amounts,
                    remind_days: st.remind_days,
                    last: pref(LAST),
                    last_error: pref(LAST_ERROR),
                    available: [Provider::Google, Provider::Outlook].into_iter().filter(|p| app_id(*p).is_some()).collect(),
                    horizon_days: HORIZON_DAYS,
                })
            }),
            "calendar.connect_start" => {
                let p: Start = params(p)?;
                self.calendar_connect_start(p.provider).await
            }
            "calendar.connect_finish" => self.calendar_connect_finish().await,
            "calendar.connect_cancel" => {
                self.calendar_pending.lock().unwrap_or_else(|e| e.into_inner()).take();
                self.calendar_cancel.notify_waiters();
                ok(())
            }
            "calendar.set" => {
                let p: SetCalendar = params(p)?;
                if p.remind_days.is_some_and(|d| d > MAX_REMIND_DAYS) {
                    return Err(ApiError::new("bad_params", format!("remind_days is at most {MAX_REMIND_DAYS}")));
                }
                self.with(|s| {
                    if let Some(a) = p.show_amounts {
                        s.set_pref(AMOUNTS, Some(if a { "1" } else { "0" }))?;
                    }
                    if let Some(d) = p.remind_days {
                        s.set_pref(REMIND_DAYS, Some(&d.to_string()))?;
                    }
                    Ok(())
                })
            }
            "calendar.sync" => self.calendar_sync().await,
            "calendar.disconnect" => self.calendar_disconnect().await,
            _ => Err(unknown_method(method)),
        }
    }

    async fn calendar_connect_start(&self, provider: Provider) -> ApiResult<Value> {
        let book = {
            let guard = self.lock();
            let book = guard.as_ref().ok_or_else(|| ApiError::new("no_book", "no book is open"))?;
            if book.store.pref(SECRET)?.is_some() {
                return Err(ApiError::new("rule", "this book already has a calendar; disconnect it first"));
            }
            book.path.clone()
        };
        let app = app_id(provider).ok_or_else(|| {
            ApiError::new("calendar_unavailable", "Calendar sign-in isn't switched on in this version of Tallybook yet.")
        })?;
        let (pending, url) = oauth::start(&app).await.map_err(|e| ApiError::new("io", format!("couldn't start the sign-in: {e}")))?;
        // A newer start replaces an older one; its listener closes with it.
        *self.calendar_pending.lock().unwrap_or_else(|e| e.into_inner()) = Some(PendingSignIn { app, pending, book });
        ok(json!({ "url": url }))
    }

    async fn calendar_connect_finish(&self) -> ApiResult<Value> {
        let sign_in = self
            .calendar_pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .ok_or_else(|| ApiError::new("rule", "start connecting a calendar first"))?;
        let code = tokio::select! {
            code = sign_in.pending.wait_for_code() => code.map_err(cal_err)?,
            _ = self.calendar_cancel.notified() => return Err(ApiError::new("cancelled", "connecting the calendar was cancelled")),
            _ = tokio::time::sleep(SIGN_IN_WAIT) => return Err(ApiError::new("calendar", "the sign-in took too long; try again")),
        };
        let tokens = oauth::exchange(&sign_in.app, &sign_in.pending, &code).await.map_err(cal_err)?;
        let refresh = tokens.refresh_token.clone().unwrap_or_default();
        let client = Client::new(sign_in.app.provider, tokens.access_token);
        let calendar_id = client.create_calendar().await.map_err(cal_err)?;
        let key = format!("calendar:{}", uuid::Uuid::new_v4());
        self.secrets.set(&key, &refresh).map_err(|e| ApiError::new("keychain", e))?;
        let saved = {
            let mut guard = self.lock();
            match guard.as_mut() {
                Some(book) if book.path == sign_in.book => (|| {
                    let s = &book.store;
                    s.set_pref(PROVIDER, Some(sign_in.app.provider.as_str()))?;
                    s.set_pref(CALENDAR_ID, Some(&calendar_id))?;
                    s.set_pref(SECRET, Some(&key))?;
                    s.set_pref(LAST_ERROR, None)
                })()
                .map_err(ApiError::from),
                _ => Err(ApiError::new("rule", "the book changed while signing in; connect the calendar again")),
            }
        };
        if let Err(e) = saved {
            let _ = self.secrets.delete(&key);
            let _ = client.delete_calendar(&calendar_id).await;
            return Err(e);
        }
        self.calendar_sync().await
    }

    /// `calendar.sync`: `{connected: false}` when the book has no calendar,
    /// else `{connected: true, added, updated, removed}`.
    async fn calendar_sync(&self) -> ApiResult<Value> {
        // One sync at a time, so two can't both add the same event.
        let _one = self.calendar_syncing.lock().await;
        let t = today();
        let Some(c) = self.run(|s| read_connection(s, t))? else {
            return ok(json!({ "connected": false }));
        };
        match self.push(&c, t).await {
            Ok((report, calendar_id)) => {
                self.run(|s| {
                    if calendar_id != c.calendar_id {
                        s.set_pref(CALENDAR_ID, Some(&calendar_id))?;
                    }
                    s.set_pref(LAST, Some(&Local::now().to_rfc3339()))?;
                    s.set_pref(LAST_ERROR, None)
                })?;
                ok(json!({ "connected": true, "added": report.added, "updated": report.updated, "removed": report.removed }))
            }
            Err(e) => {
                let _ = self.run(|s| s.set_pref(LAST_ERROR, Some(&e.message)));
                Err(e)
            }
        }
    }

    /// Signs in and makes the calendar match `c.events`. A calendar the user
    /// deleted is made again; the id it ends up with is returned.
    async fn push(&self, c: &Connection, today: NaiveDate) -> ApiResult<(tally_calendar::SyncReport, String)> {
        let app = app_id(c.provider).ok_or_else(|| ApiError::new("calendar_unavailable", "Calendar sign-in isn't switched on in this version of Tallybook."))?;
        let refresh = self
            .secrets
            .get(&c.secret_key)
            .map_err(|e| ApiError::new("keychain", e))?
            .ok_or_else(|| ApiError::new("calendar", "the calendar sign-in is missing from this computer's keychain; disconnect and connect the calendar again"))?;
        let tokens = oauth::refresh(&app, &refresh).await.map_err(cal_err)?;
        // Microsoft hands out a new refresh token now and then; keep the newest.
        if let Some(r) = tokens.refresh_token.as_deref().filter(|r| *r != refresh) {
            self.secrets.set(&c.secret_key, r).map_err(|e| ApiError::new("keychain", e))?;
        }
        let client = Client::new(c.provider, tokens.access_token);
        match client.sync(&c.calendar_id, today, &c.events).await {
            Ok(r) => Ok((r, c.calendar_id.clone())),
            Err(CalendarError::NotFound) => {
                let id = client.create_calendar().await.map_err(cal_err)?;
                let r = client.sync(&id, today, &c.events).await.map_err(cal_err)?;
                Ok((r, id))
            }
            Err(e) => Err(cal_err(e)),
        }
    }

    /// Removes Tallybook's calendar (best effort), the sign-in, and the
    /// book's calendar settings. Returns `{warnings}`.
    async fn calendar_disconnect(&self) -> ApiResult<Value> {
        let _one = self.calendar_syncing.lock().await;
        let (provider, calendar_id, key) = self.run(|s| {
            Ok((s.pref(PROVIDER)?.as_deref().and_then(Provider::parse), s.pref(CALENDAR_ID)?, s.pref(SECRET)?.as_deref().and_then(secret_key).map(str::to_string)))
        })?;
        let mut warnings = Vec::new();
        if let (Some(provider), Some(key)) = (provider, &key)
            && let (Some(app), Ok(Some(refresh))) = (app_id(provider), self.secrets.get(key))
        {
            match oauth::refresh(&app, &refresh).await {
                Ok(t) => {
                    if let Some(id) = &calendar_id
                        && let Err(e) = Client::new(provider, t.access_token).delete_calendar(id).await
                    {
                        warnings.push(format!("couldn't remove the Tallybook calendar ({e}); delete it in your calendar app"));
                    }
                }
                Err(e) => warnings.push(format!("couldn't sign in to remove the Tallybook calendar ({e}); delete it in your calendar app")),
            }
            if let Err(e) = oauth::revoke(&app, &refresh).await {
                warnings.push(e.to_string());
            }
        }
        if let Some(key) = &key
            && let Err(e) = self.secrets.delete(key)
        {
            warnings.push(format!("keychain: {e}"));
        }
        self.run(|s| {
            for k in [PROVIDER, CALENDAR_ID, SECRET, LAST, LAST_ERROR] {
                s.set_pref(k, None)?;
            }
            Ok(())
        })?;
        ok(json!({ "warnings": warnings }))
    }
}

/// The book's calendar connection and the events it should show, or `None`
/// when it has no calendar.
fn read_connection(s: &Store, today: NaiveDate) -> Result<Option<Connection>, StoreError> {
    let (Some(provider), Some(calendar_id), Some(secret)) = (s.pref(PROVIDER)?.as_deref().and_then(Provider::parse), s.pref(CALENDAR_ID)?, s.pref(SECRET)?) else {
        return Ok(None);
    };
    let Some(secret_key) = secret_key(&secret).map(str::to_string) else { return Ok(None) };
    let varies: HashMap<RecurringBillId, bool> = s.recurring_bills(today)?.into_iter().map(|b| (b.id, b.bill.varies)).collect();
    let due = s.due_dates(today, today + Days::new(HORIZON_DAYS), today)?;
    let bills = s.bills(today)?;
    Ok(Some(Connection { provider, calendar_id, secret_key, events: wanted(&due, &varies, &bills, style(s)?, today) }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tally_core::Cents;

    fn d(m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, m, day).unwrap()
    }

    fn due(bill_id: RecurringBillId, name: &str, cents: i64, on: NaiveDate, state: DueState) -> DueDate {
        DueDate { bill_id, name: name.into(), amount: Cents(cents), account_id: 1, person_id: None, auto_pay: false, due_date: on, state, payment: None }
    }

    fn bill(id: i64, number: &str, balance: i64, on: NaiveDate, status: DocStatus) -> BillSummary {
        BillSummary {
            id,
            number: number.into(),
            vendor_id: 1,
            vendor_name: "Acme Supply".into(),
            issue_date: d(10, 1),
            due_date: on,
            total: Cents(balance),
            paid: Cents(0),
            balance: Cents(balance),
            status,
            memo: String::new(),
        }
    }

    #[test]
    fn the_calendar_shows_whats_still_owed_in_the_window() {
        let today = d(10, 28);
        let household = [
            due(1, "Rent", 145_000, d(11, 1), DueState::Upcoming),
            due(2, "Power", 9_000, d(10, 28), DueState::DueToday),
            due(3, "Netflix", 1_599, d(11, 2), DueState::Paid),
            due(4, "Gym", 4_000, d(11, 3), DueState::Skipped),
            due(5, "Old", 100, d(10, 20), DueState::Overdue),
            due(6, "Far off", 100, today + Days::new(HORIZON_DAYS + 1), DueState::Upcoming),
        ];
        let business = [
            bill(7, "1042", 120_000, d(11, 15), DocStatus::Open),
            bill(8, "", 5_000, d(11, 16), DocStatus::Partial),
            bill(9, "1043", 0, d(11, 17), DocStatus::Paid),
            bill(10, "1044", 3_000, d(11, 18), DocStatus::Draft),
            bill(11, "1045", 3_000, d(11, 19), DocStatus::Void),
        ];
        let varies = HashMap::from([(2, true)]);
        let quiet = Style { show_amounts: false, remind_days: 1 };
        let got = wanted(&household, &varies, &business, quiet, today);
        let titles: Vec<(&str, &str)> = got.iter().map(|e| (e.key.as_str(), e.title.as_str())).collect();
        assert_eq!(
            titles,
            [
                ("rb:2:2026-10-28", "Due: Power"),
                ("rb:1:2026-11-01", "Due: Rent"),
                ("bill:7", "Due: Acme Supply (bill 1042)"),
                ("bill:8", "Due: Acme Supply"),
            ]
        );
        // 9 am the day before: 24 hours less 15 before midnight.
        assert!(got.iter().all(|e| e.reminder_minutes == Some(9 * 60)));

        let loud = Style { show_amounts: true, remind_days: 0 };
        let got = wanted(&household, &varies, &business, loud, today);
        let titles: Vec<&str> = got.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["Due: Power — about $90.00", "Due: Rent — $1,450.00", "Due: Acme Supply (bill 1042) — $1,200.00", "Due: Acme Supply — $50.00"]);
        assert!(got.iter().all(|e| e.reminder_minutes.is_none()));
    }

    #[test]
    fn names_go_on_one_line_and_are_cut_short() {
        assert_eq!(clean("  Rent\n\tdue  "), "Rent due");
        let long = clean(&"x".repeat(200));
        assert_eq!(long.chars().count(), MAX_NAME);
        assert!(long.ends_with('…'));
    }

    #[test]
    fn only_keychain_entries_tallybook_made_are_used() {
        let id = uuid::Uuid::new_v4().hyphenated().to_string();
        let ok = format!("calendar:{id}");
        assert_eq!(secret_key(&ok), Some(ok.as_str()));
        for bad in [format!("plaid:{id}"), format!("calendar:{}", id.to_uppercase()), "calendar:../book-key:1".into(), "book-key:1".into()] {
            assert_eq!(secret_key(&bad), None, "{bad}");
        }
    }
}
