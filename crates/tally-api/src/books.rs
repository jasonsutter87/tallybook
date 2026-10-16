//! Book lifecycle beyond open/create: forgetting and deleting (teardown).

use crate::{ApiError, ApiResult, App, ok, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use tally_feed::plaid;
use tally_store::{Provider, Store};

#[derive(Deserialize)]
struct DeleteBook {
    path: PathBuf,
    /// Must equal the book's business name; a guard against deleting the wrong file.
    confirm_name: String,
    #[serde(default)]
    include_backups: bool,
}

#[derive(Serialize, Default)]
struct DeleteReport {
    trashed: Vec<PathBuf>,
    keychain_entries_removed: usize,
    bank_connections_revoked: usize,
    warnings: Vec<String>,
}

/// The book file and SQLite's write-ahead companions.
fn book_files(path: &Path) -> Vec<PathBuf> {
    let mut files = vec![path.to_path_buf()];
    for suffix in ["-wal", "-shm"] {
        let mut p = path.as_os_str().to_owned();
        p.push(suffix);
        let p = PathBuf::from(p);
        if p.exists() {
            files.push(p);
        }
    }
    files
}

/// Dated backups made by `book.backup`: `<folder>/Backups/<stem> <timestamp>.tallybook`.
fn backup_files(path: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(stem)) = (path.parent(), path.file_stem().and_then(|s| s.to_str())) else {
        return vec![];
    };
    let prefix = format!("{stem} ");
    std::fs::read_dir(dir.join("Backups"))
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| {
                    p.extension().is_some_and(|e| e == "tallybook")
                        && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(&prefix))
                })
                .collect()
        })
        .unwrap_or_default()
}

impl App {
    pub(crate) async fn books(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "book.delete" => {
                let p: DeleteBook = params(p)?;
                self.delete_book(p).await
            }
            _ => Err(ApiError::new("unknown_method", format!("no such method: {method}"))),
        }
    }

    /// Tears a book down: revokes bank access, removes its keychain entries,
    /// and moves the file (and optionally its backups) to the Trash.
    async fn delete_book(&self, p: DeleteBook) -> ApiResult<Value> {
        if p.path.extension().is_none_or(|e| e != "tallybook") {
            return Err(ApiError::new("rule", "that isn't a Tallybook book file"));
        }
        if !p.path.exists() {
            return Err(ApiError::new("rule", format!("{} doesn't exist", p.path.display())));
        }
        let path = p.path.canonicalize().unwrap_or(p.path.clone());

        // Close it if it's the open book, then read what needs revoking.
        {
            let mut guard = self.lock();
            if guard.as_ref().is_some_and(|b| b.path == path) {
                *guard = None;
            }
        }
        let (name, connections) = {
            let store = Store::open(&path)?;
            (store.pref("company.name")?.unwrap_or_default(), store.connections_with_secrets()?)
        };
        if p.confirm_name.trim() != name.trim() {
            return Err(ApiError::new("rule", format!("type the business name exactly (\"{name}\") to confirm")));
        }

        let mut report = DeleteReport::default();
        for conn in &connections {
            let Some(key) = &conn.secret_ref else { continue };
            if conn.provider == Provider::Plaid
                && let Ok(Some(token)) = self.secrets.get(key)
            {
                // Best effort: tell Plaid to drop its access to the bank.
                match self.plaid_client_opt() {
                    Some(client) => match client.remove_item(&token).await {
                        Ok(()) => report.bank_connections_revoked += 1,
                        Err(e) => report.warnings.push(format!("{}: couldn't revoke at Plaid ({e})", conn.institution)),
                    },
                    None => report.warnings.push(format!("{}: Plaid keys missing, couldn't revoke", conn.institution)),
                }
            }
            match self.secrets.delete(key) {
                Ok(()) => report.keychain_entries_removed += 1,
                Err(e) => report.warnings.push(format!("keychain: {e}")),
            }
        }

        let mut files = book_files(&path);
        if p.include_backups {
            files.extend(backup_files(&path));
        }
        for f in files {
            match (self.remove_file)(&f) {
                Ok(()) => report.trashed.push(f),
                Err(e) => report.warnings.push(format!("{}: {e}", f.display())),
            }
        }

        let mut recent = self.recent();
        recent.retain(|r| r.path != path);
        self.save_recent(&recent);
        ok(report)
    }

    fn plaid_client_opt(&self) -> Option<plaid::Client> {
        let raw = self.secrets.get("plaid.credentials").ok().flatten()?;
        serde_json::from_str::<plaid::Credentials>(&raw).ok().map(plaid::Client::new)
    }
}

/// Default file remover: the OS Trash, so a mistaken delete can be undone.
pub fn move_to_trash(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let ctx = {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        // The default asks Finder via AppleScript: slow, and macOS prompts
        // "wants to control Finder". The file-manager API needs no permission.
        let mut ctx = trash::TrashContext::default();
        ctx.set_delete_method(DeleteMethod::NsFileManager);
        ctx
    };
    #[cfg(not(target_os = "macos"))]
    let ctx = trash::TrashContext::default();
    ctx.delete(path).map_err(|e| std::io::Error::other(e.to_string()))
}
