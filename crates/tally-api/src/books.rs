//! Book lifecycle beyond open/create: forgetting and deleting (teardown).

use crate::{ApiError, ApiResult, App, ok, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use tally_feed::plaid;
use tally_store::crypto::{self, BookKey};
use tally_store::{Provider, Store, StoreError};

#[derive(Deserialize)]
struct DeleteBook {
    path: PathBuf,
    /// Must equal the book's business name; a guard against deleting the wrong file.
    confirm_name: String,
    #[serde(default)]
    include_backups: bool,
}

#[derive(Deserialize)]
struct Unlock {
    path: PathBuf,
    recovery_key: String,
    /// Save the key in the keychain so this book opens without it next time.
    #[serde(default = "yes")]
    remember: bool,
}

fn yes() -> bool {
    true
}

#[derive(Serialize)]
struct Encrypted {
    recovery_key: String,
    /// Dated backups made before encryption, still readable as plain files.
    plain_backups: usize,
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

/// `path` with a suffix appended (not replacing the extension).
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

impl App {
    // ------------------------------------------------------------ key registry
    //
    // Book keys live in the OS keychain as "book-key:<id>". The ids (not the
    // keys) are listed in keys.json so we know what to try. Keys aren't tied
    // to file paths: an encrypted book, its backups and its cloud copies all
    // open on any machine that has the key.

    fn key_ids(&self) -> Vec<String> {
        std::fs::read(self.config_dir.join("keys.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn known_keys(&self) -> Vec<BookKey> {
        self.key_ids()
            .iter()
            .filter_map(|id| self.secrets.get(&format!("book-key:{id}")).ok().flatten())
            .filter_map(|h| crypto::unhex(&h))
            .collect()
    }

    fn remember_key(&self, key: &BookKey) -> ApiResult<()> {
        if self.known_keys().contains(key) {
            return Ok(());
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.secrets.set(&format!("book-key:{id}"), &crypto::hex(key)).map_err(|e| ApiError::new("keychain", e))?;
        let mut ids = self.key_ids();
        ids.push(id);
        crate::write_private(&self.config_dir.join("keys.json"), &serde_json::to_vec(&ids).unwrap_or_default())
            .map_err(|e| ApiError::new("io", e.to_string()))
    }

    /// Opens any book: plain ones directly, encrypted ones with a known key.
    pub(crate) fn open_any(&self, path: &Path) -> ApiResult<Store> {
        match Store::open(path) {
            Err(StoreError::Locked) => {
                for key in self.known_keys() {
                    if let Ok(store) = Store::open_encrypted(path, &key) {
                        return Ok(store);
                    }
                }
                Err(StoreError::Locked.into())
            }
            other => Ok(other?),
        }
    }

    // ------------------------------------------------------------ methods

    pub(crate) async fn books(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "book.delete" => {
                let p: DeleteBook = params(p)?;
                self.delete_book(p).await
            }
            "book.unlock" => {
                let p: Unlock = params(p)?;
                let key = crypto::parse_recovery_key(&p.recovery_key)?;
                let store = Store::open_encrypted(&p.path, &key)?;
                if p.remember {
                    self.remember_key(&key)?;
                }
                self.install(store, &p.path)
            }
            "book.encrypt" => self.encrypt_open_book(),
            "book.decrypt" => self.decrypt_open_book(),
            "book.recovery_key" => {
                let guard = self.lock();
                let book = guard.as_ref().ok_or_else(|| ApiError::new("no_book", "no book is open"))?;
                let key = book.store.key().ok_or_else(|| ApiError::new("rule", "this book isn't encrypted"))?;
                ok(crypto::recovery_key(key))
            }
            "book.delete_plain_backups" => {
                let path = self.lock().as_ref().map(|b| b.path.clone()).ok_or_else(|| ApiError::new("no_book", "no book is open"))?;
                let mut removed = 0;
                for f in backup_files(&path) {
                    if !crypto::is_encrypted_file(&f).unwrap_or(true) {
                        // Permanently: a plaintext copy in the Trash defeats the point.
                        std::fs::remove_file(&f).map_err(|e| ApiError::new("io", e.to_string()))?;
                        removed += 1;
                    }
                }
                ok(removed)
            }
            _ => Err(crate::unknown_method(method)),
        }
    }

    /// Tears a book down: revokes bank access, removes its keychain entries,
    /// and moves the file (and optionally its backups) to the Trash.
    async fn delete_book(&self, p: DeleteBook) -> ApiResult<Value> {
        if p.path.extension().is_none_or(|e| e != "tallybook") {
            return Err(ApiError::new("rule", "that isn't a Tallybook book file"));
        }
        if !p.path.exists() {
            return Err(ApiError::new("not_found", format!("{} doesn't exist", p.path.display())));
        }
        let path = p.path.canonicalize().unwrap_or(p.path.clone());

        // Take the book out of the app if it's the open one, but keep it alive until the
        // name is confirmed: a wrong confirmation must leave the user's book open.
        let was_open = {
            let mut guard = self.lock();
            if guard.as_ref().is_some_and(|b| b.path == path) { guard.take() } else { None }
        };
        let restore = |book: Option<crate::OpenBook>| {
            if book.is_some() {
                *self.lock() = book;
            }
        };
        let (name, connections) = match self.open_any(&path).and_then(|store| Ok((store.pref("company.name")?.unwrap_or_default(), store.connections_with_secrets()?))) {
            Ok(read) => read,
            // Encrypted and no key on this machine: we can't read it, so confirm
            // with the file name instead, and there are no keychain entries to clean.
            Err(e) if e.code == "locked" => (path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string(), vec![]),
            Err(e) => {
                restore(was_open);
                return Err(e);
            }
        };
        if p.confirm_name.trim() != name.trim() {
            restore(was_open);
            return Err(ApiError::new("rule", format!("type the business name exactly (\"{name}\") to confirm")));
        }
        // Confirmed: close it for real before the files are removed.
        drop(was_open);

        let mut report = DeleteReport::default();
        for conn in &connections {
            let Some(key) = crate::sync::secret_key(conn) else { continue };
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

    /// Converts the open book to an encrypted one in place.
    fn encrypt_open_book(&self) -> ApiResult<Value> {
        let path = {
            let guard = self.lock();
            let book = guard.as_ref().ok_or_else(|| ApiError::new("no_book", "no book is open"))?;
            if book.store.is_encrypted() {
                return Err(ApiError::new("rule", "this book is already encrypted"));
            }
            book.path.clone()
        };
        let key = crypto::generate_key()?;
        // Remember the key before anything changes, so a crash can't strand the book.
        self.remember_key(&key)?;
        self.convert(&path, |store, tmp| store.export_encrypted(tmp, &key), |tmp| Store::open_encrypted(tmp, &key))?;
        let plain_backups = backup_files(&path).iter().filter(|f| !crypto::is_encrypted_file(f).unwrap_or(true)).count();
        ok(Encrypted { recovery_key: crypto::recovery_key(&key), plain_backups })
    }

    fn decrypt_open_book(&self) -> ApiResult<Value> {
        let path = {
            let guard = self.lock();
            let book = guard.as_ref().ok_or_else(|| ApiError::new("no_book", "no book is open"))?;
            if !book.store.is_encrypted() {
                return Err(ApiError::new("rule", "this book isn't encrypted"));
            }
            book.path.clone()
        };
        self.convert(&path, |store, tmp| store.export_plain(tmp), |tmp| Store::open(tmp))?;
        ok(())
    }

    /// Writes a converted copy next to the book, verifies it holds the same
    /// data, then swaps it in. The original is removed only after the new
    /// file is in place and opens.
    fn convert(
        &self,
        path: &Path,
        export: impl FnOnce(&Store, &Path) -> Result<(), StoreError>,
        reopen: impl Fn(&Path) -> Result<Store, StoreError>,
    ) -> ApiResult<()> {
        let tmp = sibling(path, ".converting");
        let old = sibling(path, ".previous");
        let _ = std::fs::remove_file(&tmp);
        let mut guard = self.lock();
        let book = guard.as_mut().ok_or_else(|| ApiError::new("no_book", "no book is open"))?;
        let before = book.store.fingerprint()?;
        export(&book.store, &tmp)?;
        let after = reopen(&tmp)?.fingerprint()?;
        if before != after {
            let _ = std::fs::remove_file(&tmp);
            return Err(ApiError::new("internal", "the converted copy didn't match; your book was left unchanged"));
        }
        // Close the original (checkpoints its WAL), then swap files.
        let original = guard.take().expect("checked above");
        let original_path = original.path.clone();
        drop(original);
        // If the swap can't happen, put the original back open and say why in plain words.
        // Windows refuses while another process has the file open: another Tallybook,
        // or an agent connected with `--mcp`.
        let restore = |guard: &mut Option<crate::OpenBook>, e: std::io::Error| -> ApiError {
            let _ = std::fs::remove_file(&tmp);
            if let Ok(store) = self.open_any(path) {
                *guard = Some(crate::OpenBook::new(store, original_path.clone()));
            }
            ApiError::new(
                "io",
                format!(
                    "couldn't replace the book file, so nothing was changed. It may be open in another copy of Tallybook or an AI agent; close it there and try again. ({e})"
                ),
            )
        };
        if let Err(e) = std::fs::rename(path, &old) {
            return Err(restore(&mut guard, e));
        }
        if let Err(e) = std::fs::rename(&tmp, path) {
            let _ = std::fs::rename(&old, path);
            return Err(restore(&mut guard, e));
        }
        for f in book_files(&old) {
            let _ = std::fs::remove_file(f);
        }
        for suffix in ["-wal", "-shm"] {
            let _ = std::fs::remove_file(sibling(path, suffix));
        }
        let store = reopen(path)?;
        *guard = Some(crate::OpenBook::new(store, path.to_path_buf()));
        Ok(())
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
