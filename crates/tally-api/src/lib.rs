//! Tallybook's application layer.
//!
//! The UI talks to exactly one function, [`App::call`]: a method name such as
//! `"invoices.create"` plus JSON params, returning JSON. The desktop shell
//! (Tauri) and the browser dev server both just forward to it, so the two can
//! never drift apart.
//!
//! Money crosses the boundary as integer cents; dates as `YYYY-MM-DD`;
//! budget months as `YYYY-MM`.
//!
//! [`App::call`] only routes: it picks a module by the part of the method name
//! before the dot, and each of these has the `match` listing its methods (and its
//! doc comment names them): `session.rs` (`book.*`), `books.rs` (book
//! encryption and deletion), `settings.rs`, `reports.rs` (`dashboard`,
//! `reports.*`), `accounts.rs` (`accounts.*`, `entries.*`), `people.rs`
//! (`people.*`, `budget.*`), `recurring.rs`, `goals.rs` (`goals.*`,
//! `household.*`), `inbox.rs`, `import.rs`, `rules.rs`, `reconcile.rs`,
//! `contacts.rs`, `invoices.rs`, `bills.rs` (`bills.*`, `payments.*`), `sync.rs`
//! (`bank.*`), `tax_api.rs` (`tax.*`), `license.rs` and `cloud.rs`. Each
//! method's params are a `#[derive(Deserialize)]` struct beside its `match`
//! (or a store type taken whole); missing `Option` fields take
//! the defaults noted beside them, usually "today". A book has one or two
//! sides (`"business"`, `"household"`); methods that take an optional `scope`
//! cover the whole book when it is absent, unless noted otherwise.

mod accounts;
mod bills;
mod books;
mod cloud;
mod contacts;
mod dashboard;
mod demo;
mod goals;
mod import;
mod inbox;
mod invoices;
mod license;
mod pdf;
mod people;
mod reconcile;
mod recurring;
mod reports;
mod rules;
pub mod secrets;
mod session;
mod settings;
mod sync;
mod tax_api;
mod tax_pdf;

use chrono::{Local, NaiveDate};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use tally_core::Scope;
use tally_store::*;
// The glob above brings in the store's one-arg `Result`; keep std's.
use std::result::Result;

pub use secrets::{Keychain, MemorySecrets, Secrets};

/// Error shape the UI receives. `code` is machine-readable; `message` is
/// written for humans and safe to show as-is.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ApiError {
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn new(code: &'static str, message: impl Into<String>) -> ApiError {
        ApiError { code, message: message.into() }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> ApiError {
        match e {
            StoreError::Rule(m) => ApiError::new("rule", m),
            StoreError::Entry(e) => ApiError::new("rule", e.to_string()),
            StoreError::NotFound(..) => ApiError::new("not_found", e.to_string()),
            StoreError::Locked => ApiError::new("locked", "this book is encrypted; enter its recovery key to open it"),
            StoreError::Sqlite(e) => ApiError::new("internal", format!("database error: {e}")),
        }
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

fn params<T: DeserializeOwned>(v: Value) -> ApiResult<T> {
    // Methods without params may be called with `null`.
    let v = if v.is_null() { json!({}) } else { v };
    serde_json::from_value(v).map_err(|e| ApiError::new("bad_params", e.to_string()))
}

/// The error every area's router returns for a method it doesn't have.
fn unknown_method(method: &str) -> ApiError {
    ApiError::new("unknown_method", format!("no such method: {method}"))
}

fn ok<T: Serialize>(v: T) -> ApiResult<Value> {
    serde_json::to_value(v).map_err(|e| ApiError::new("internal", e.to_string()))
}

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

pub(crate) struct OpenBook {
    pub(crate) store: Store,
    pub(crate) path: PathBuf,
    /// Which file on disk was opened, to notice when another process swaps it.
    file_id: Option<(u64, u64)>,
}

impl OpenBook {
    pub(crate) fn new(store: Store, path: PathBuf) -> OpenBook {
        OpenBook { store, file_id: file_id(&path), path }
    }
}

/// The file's identity on disk (device and inode): it changes when the file is
/// replaced, not when it's written to. `None` where that isn't available; on
/// Windows a book can't be replaced while it's open anyway.
#[cfg(unix)]
fn file_id(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
}

#[cfg(not(unix))]
fn file_id(_path: &Path) -> Option<(u64, u64)> {
    None
}

pub struct App {
    book: Mutex<Option<OpenBook>>,
    secrets: Arc<dyn Secrets>,
    /// Where `recent.json` lives (the OS app-config dir).
    config_dir: PathBuf,
    /// How deleted books leave the disk: the OS Trash, or plain removal in tests.
    remove_file: fn(&Path) -> std::io::Result<()>,
}

// ---------------------------------------------------------------- shared param types
//
// Used by several areas; each area's own params sit beside its `match`.

#[derive(Deserialize)]
struct Id {
    id: i64,
}

#[derive(Deserialize)]
struct IdDate {
    id: i64,
    date: Option<NaiveDate>,
}

#[derive(Deserialize)]
struct OpenPath {
    path: PathBuf,
}

#[derive(Deserialize)]
struct Archive {
    id: i64,
    archived: bool,
}

/// `budget.month` and `household.*`: a `YYYY-MM` month; the current one when absent.
#[derive(Deserialize)]
struct MonthParam {
    month: Option<String>,
}

// ---------------------------------------------------------------- app

impl App {
    pub fn new(secrets: Arc<dyn Secrets>, config_dir: impl Into<PathBuf>) -> App {
        App { book: Mutex::new(None), secrets, config_dir: config_dir.into(), remove_file: books::move_to_trash }
    }

    /// Replaces the Trash with permanent removal (tests, headless servers).
    pub fn with_file_remover(mut self, remove: fn(&Path) -> std::io::Result<()>) -> App {
        self.remove_file = remove;
        self
    }

    fn lock(&self) -> MutexGuard<'_, Option<OpenBook>> {
        // A panic while holding the lock can't corrupt SQLite (transactions
        // roll back), so recover rather than wedge the app.
        self.book.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Runs `f` against the open book, returning its typed result.
    fn run<T>(&self, f: impl FnOnce(&mut Store) -> Result<T, StoreError>) -> ApiResult<T> {
        let mut guard = self.lock();
        let book = guard.as_mut().ok_or_else(|| ApiError::new("no_book", "no book is open"))?;
        Ok(f(&mut book.store)?)
    }

    /// Runs `f` against the open book, returning JSON.
    fn with<T: Serialize>(&self, f: impl FnOnce(&mut Store) -> Result<T, StoreError>) -> ApiResult<Value> {
        ok(self.run(f)?)
    }

    fn with_api<T: Serialize>(&self, f: impl FnOnce(&mut Store) -> ApiResult<T>) -> ApiResult<Value> {
        let mut guard = self.lock();
        let book = guard.as_mut().ok_or_else(|| ApiError::new("no_book", "no book is open"))?;
        ok(f(&mut book.store)?)
    }

    fn call_status(&self) -> ApiResult<Value> {
        let guard = self.lock();
        let book = guard.as_ref().ok_or_else(|| ApiError::new("no_book", "no book is open"))?;
        ok(json!({ "path": book.path, "company_name": book.store.pref("company.name")? }))
    }

    pub fn is_open(&self) -> bool {
        self.lock().is_some()
    }

    /// The desktop app can swap the book file for an encrypted or decrypted copy
    /// while an agent (tally-mcp) has it open. Without this, the agent would keep
    /// reading and writing the replaced file, and its changes would be lost.
    fn follow_replaced_book(&self) {
        let mut guard = self.lock();
        let Some(book) = guard.as_mut() else { return };
        let now = file_id(&book.path);
        if now.is_none() || now == book.file_id {
            return;
        }
        match self.open_any(&book.path) {
            Ok(store) => {
                book.store = store;
                book.file_id = now;
            }
            // Can't open what's there now (no key for it here, say): stop using
            // the replaced file, so the next call says "no book is open" rather
            // than writing somewhere nobody will ever read.
            Err(_) => *guard = None,
        }
    }

    /// The single entry point. Each area's methods live in their own module
    /// (see the crate docs); this only picks which one by the part of the name
    /// before the dot.
    pub async fn call(&self, method: &str, p: Value) -> ApiResult<Value> {
        self.follow_replaced_book();
        match method {
            // Teardown and encryption share the `book.` prefix with the
            // everyday ones in `session.rs`, so they're matched by name.
            "book.delete" | "book.unlock" | "book.encrypt" | "book.decrypt" | "book.recovery_key" | "book.delete_plain_backups" => {
                self.books(method, p).await
            }
            // Counting a goal is an `entries.` method but belongs with goals.
            "entries.set_goal" => self.goals(method, p),
            _ => match method.split('.').next().unwrap_or(method) {
                "book" => self.session(method, p),
                "settings" => self.settings(method, p),
                "dashboard" | "reports" => self.reports(method, p),
                "accounts" | "entries" => self.accounts(method, p),
                "people" | "budget" => self.people(method, p),
                "recurring" => self.recurring(method, p),
                "goals" | "household" => self.goals(method, p),
                "inbox" => self.inbox(method, p),
                "import" => self.import(method, p),
                "rules" => self.rules(method, p),
                "reconcile" => self.reconcile(method, p),
                "contacts" => self.contacts(method, p),
                "invoices" => self.invoices(method, p),
                "bills" | "payments" => self.bills(method, p),
                "bank" => self.bank(method, p).await,
                "tax" => self.tax(method, p).await,
                "license" => self.license(method, p).await,
                "cloud" => self.cloud(method, p).await,
                _ => Err(unknown_method(method)),
            },
        }
    }
}

/// The side a book is mostly about: business when it keeps one.
pub(crate) fn main_side(s: &Store) -> Result<Scope, StoreError> {
    Ok(s.sides()?.first().copied().unwrap_or_default())
}

/// `~/Documents/Tallybook`, or `$TALLYBOOK_DIR` when set (development, tests).
pub fn default_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("TALLYBOOK_DIR") {
        return PathBuf::from(dir);
    }
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join("Documents").join("Tallybook")
}

#[cfg(test)]
mod tests;
