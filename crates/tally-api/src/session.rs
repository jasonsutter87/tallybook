//! Opening, creating, closing and backing up books, and the recent-books list the welcome screen shows.
//!
//! Methods: `book.status`, `book.create`, `book.create_demo`, `book.open`,
//! `book.close`, `book.recent`, `book.forget`, `book.default_dir`,
//! `book.add_side`, `book.backup`. Encrypting and deleting books live in
//! `books.rs`.

use crate::{ApiError, ApiResult, App, OpenBook, OpenPath, default_dir, demo, license, ok, params, today, unknown_method};
use chrono::{Local, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tally_core::Scope;
use tally_store::*;

/// How many books the welcome screen remembers in `recent.json`.
const RECENT_BOOKS_KEPT: usize = 10;

#[derive(Deserialize)]
struct CreateBook {
    path: PathBuf,
    company_name: String,
    /// Which sides to keep; business only when absent.
    #[serde(default)]
    sides: Option<Vec<Scope>>,
}

/// `book.add_side`: adding a side the book already has is a no-op.
#[derive(Deserialize)]
struct AddSide {
    scope: Scope,
}

#[derive(Serialize)]
struct BookStatus {
    open: bool,
    path: Option<PathBuf>,
    company_name: Option<String>,
    encrypted: bool,
    today: NaiveDate,
    /// Sides this book keeps; empty when no book is open.
    sides: Vec<Scope>,
}

/// One entry of `recent.json` in the app-config dir, a JSON array of these,
/// newest first (see docs/FILE_FORMAT.md).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RecentBook {
    pub(crate) path: PathBuf,
    name: String,
    /// RFC 3339, local time.
    last_opened: String,
    /// Computed when listing, from the file header.
    #[serde(default)]
    encrypted: bool,
}

impl App {
    pub(crate) fn session(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "book.status" => {
                let guard = self.lock();
                let (path, name, encrypted, sides) = match guard.as_ref() {
                    Some(b) => (Some(b.path.clone()), b.store.pref("company.name")?, b.store.is_encrypted(), b.store.sides()?),
                    None => (None, None, false, vec![]),
                };
                ok(BookStatus { open: guard.is_some(), path, company_name: name, encrypted, today: today(), sides })
            }
            "book.create" => {
                let p: CreateBook = params(p)?;
                let sides = p.sides.unwrap_or_else(|| vec![Scope::Business]);
                if sides.contains(&Scope::Business) {
                    self.require(license::Tier::Business)?;
                }
                self.create_book(&p.path, &p.company_name, &sides)
            }
            "book.create_demo" => {
                let p: OpenPath = params(p)?;
                self.create_book(&p.path, "Northwind Studio", &[Scope::Business])?;
                self.with(|s| demo::seed(s, today()))?;
                self.call_status()
            }
            "book.open" => {
                let p: OpenPath = params(p)?;
                self.open_book(&p.path)
            }
            "book.close" => {
                *self.lock() = None;
                ok(())
            }
            "book.recent" => ok(self.recent()),
            "book.forget" => {
                let p: OpenPath = params(p)?;
                let mut recent = self.recent();
                recent.retain(|r| r.path != p.path);
                self.save_recent(&recent);
                ok(())
            }
            "book.default_dir" => ok(default_dir()),
            // {scope} -> null. Adds a side and its default chart.
            "book.add_side" => {
                let p: AddSide = params(p)?;
                if p.scope == Scope::Business {
                    self.require(license::Tier::Business)?;
                }
                self.with(|s| s.add_side(p.scope))
            }
            "book.backup" => self.backup(),
            _ => Err(unknown_method(method)),
        }
    }

    fn create_book(&self, path: &Path, company: &str, sides: &[Scope]) -> ApiResult<Value> {
        if company.trim().is_empty() {
            return Err(ApiError::new("rule", "give your book a name"));
        }
        if sides.is_empty() {
            return Err(ApiError::new("rule", "a book keeps a business, a household, or both"));
        }
        if path.exists() {
            return Err(ApiError::new("rule", format!("{} already exists", path.display())));
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| ApiError::new("io", e.to_string()))?;
        }
        let mut store = Store::open(path)?;
        for side in sides {
            store.add_side(*side)?;
        }
        store.set_pref("company.name", Some(company.trim()))?;
        self.install(store, path)
    }

    fn open_book(&self, path: &Path) -> ApiResult<Value> {
        if !path.exists() {
            return Err(ApiError::new("not_found", format!("{} doesn't exist", path.display())));
        }
        let store = self.open_any(path)?;
        self.install(store, path)
    }

    pub(crate) fn install(&self, store: Store, path: &Path) -> ApiResult<Value> {
        let name = store.pref("company.name")?.unwrap_or_else(|| "My Business".into());
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        *self.lock() = Some(OpenBook::new(store, path.clone()));
        // Agent sessions change no files besides the book's own changes.
        if !self.update_recent.load(std::sync::atomic::Ordering::Relaxed) {
            return ok(json!({ "path": path, "company_name": name }));
        }
        crate::books::remove_conversion_leftovers(&path);
        let mut recent = self.recent();
        recent.retain(|r| r.path != path);
        recent.insert(0, RecentBook { path: path.clone(), name: name.clone(), last_opened: Local::now().to_rfc3339(), encrypted: false });
        recent.truncate(RECENT_BOOKS_KEPT);
        self.save_recent(&recent);
        ok(json!({ "path": path, "company_name": name }))
    }

    pub(crate) fn recent(&self) -> Vec<RecentBook> {
        std::fs::read(self.config_dir.join("recent.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Vec<RecentBook>>(&b).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.path.exists())
            .map(|mut r| {
                r.encrypted = tally_store::crypto::is_encrypted_file(&r.path).unwrap_or(false);
                r
            })
            .collect()
    }

    pub(crate) fn save_recent(&self, recent: &[RecentBook]) {
        // Best effort: losing the recent list is an inconvenience, not data loss.
        if let Ok(json) = serde_json::to_vec_pretty(recent) {
            let _ = crate::write_private(&self.config_dir.join("recent.json"), &json);
        }
    }

    fn backup(&self) -> ApiResult<Value> {
        let guard = self.lock();
        let book = guard.as_ref().ok_or_else(|| ApiError::new("no_book", "no book is open"))?;
        let dir = book.path.parent().unwrap_or(Path::new(".")).join("Backups");
        std::fs::create_dir_all(&dir).map_err(|e| ApiError::new("io", e.to_string()))?;
        let stem = book.path.file_stem().and_then(|s| s.to_str()).unwrap_or("book");
        let stamp = Local::now().format("%Y-%m-%d %H%M%S").to_string();
        // Two backups in the same second get " (2)", " (3)"… rather than failing.
        let dest = (1..)
            .map(|n| dir.join(if n == 1 { format!("{stem} {stamp}.tallybook") } else { format!("{stem} {stamp} ({n}).tallybook") }))
            .find(|p| !p.exists())
            .expect("an unused name exists");
        book.store.backup_to(&dest)?;
        ok(dest)
    }
}
