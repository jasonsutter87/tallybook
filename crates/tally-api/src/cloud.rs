//! Off-machine copies of an encrypted book: into a synced folder (Google
//! Drive, iCloud Drive, Dropbox, OneDrive desktop folders) and/or a Git
//! repository pushed with the user's own git credentials.
//!
//! Only encrypted books can be copied, and every file is checked to be
//! encrypted before it is written to a destination. Nothing readable ever
//! leaves the machine.

use crate::{ApiError, ApiResult, App, ok, params, unknown_method};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use tally_store::Store;
use tally_store::crypto::is_encrypted_file;

const DEFAULT_KEEP: usize = 10;
/// How many dated copies a folder may keep, at most.
const MAX_KEEP: usize = 100;
const AUTO_EVERY_HOURS: i64 = 24;

#[derive(Serialize)]
struct Suggestion {
    label: String,
    path: PathBuf,
}

#[derive(Serialize)]
struct CloudStatus {
    encrypted: bool,
    folder: Option<String>,
    git: Option<String>,
    git_has_remote: bool,
    auto: bool,
    keep: usize,
    last: Option<String>,
    last_error: Option<String>,
    /// Auto-backup is on and the last copy is older than a day.
    due: bool,
    suggestions: Vec<Suggestion>,
}

#[derive(Deserialize)]
struct CloudSet {
    /// `Some(None)` clears a destination; omitted leaves it unchanged.
    #[serde(default, deserialize_with = "some")]
    folder: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    git: Option<Option<String>>,
    auto: Option<bool>,
    keep: Option<usize>,
}

fn some<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

#[derive(Serialize, Default)]
struct DestResult {
    ok: bool,
    path: Option<PathBuf>,
    detail: Option<String>,
}

#[derive(Serialize, Default)]
struct BackupReport {
    folder: Option<DestResult>,
    git: Option<DestResult>,
}

/// The `cloud.keep` setting as stored, read back within 1..=[`MAX_KEEP`]:
/// a folder always keeps at least the copy just made.
fn keep_from(stored: Option<String>) -> usize {
    stored.and_then(|k| k.trim().parse::<usize>().ok()).unwrap_or(DEFAULT_KEEP).clamp(1, MAX_KEEP)
}

/// A hooks folder that can't hold any hooks.
const NO_HOOKS: &str = if cfg!(windows) { "core.hooksPath=NUL" } else { "core.hooksPath=/dev/null" };

/// Runs git in `repo`. Tallybook only adds, commits and pushes one file, so it
/// runs no hooks and no filesystem monitor the repository may configure.
fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(["-c", NO_HOOKS, "-c", "core.fsmonitor=false"])
        .arg("-C")
        .arg(repo)
        .args(args)
        // Never wait for a username/password prompt the user can't see.
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| format!("couldn't run git ({e}); is it installed?"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn is_git_repo(path: &Path) -> bool {
    git(path, &["rev-parse", "--is-inside-work-tree"]).is_ok_and(|o| o == "true")
}

fn has_remote(path: &Path) -> bool {
    git(path, &["remote"]).is_ok_and(|o| !o.is_empty())
}

/// Desktop sync folders that exist on this machine.
fn suggestions() -> Vec<Suggestion> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return vec![] };
    let mut out = Vec::new();
    let mut add = |label: String, p: PathBuf| {
        if p.is_dir() && !out.iter().any(|s: &Suggestion| s.path == p) {
            out.push(Suggestion { label, path: p });
        }
    };
    if let Ok(rd) = std::fs::read_dir(home.join("Library/CloudStorage")) {
        for e in rd.filter_map(Result::ok) {
            let name = e.file_name().to_string_lossy().to_string();
            let p = e.path();
            if name.starts_with("GoogleDrive") {
                let my = p.join("My Drive");
                add(format!("Google Drive ({})", name.trim_start_matches("GoogleDrive-")), if my.is_dir() { my } else { p });
            } else if name.starts_with("OneDrive") {
                add("OneDrive".into(), p);
            } else if name.starts_with("Dropbox") {
                add("Dropbox".into(), p);
            } else if name.starts_with("Box") {
                add("Box".into(), p);
            } else if !name.starts_with('.') {
                // Any other File Provider (Nextcloud, pCloud, Proton Drive…): "Nextcloud-host-user" → "Nextcloud".
                add(name.split('-').next().unwrap_or(&name).to_string(), p);
            }
        }
    }
    add("iCloud Drive".into(), home.join("Library/Mobile Documents/com~apple~CloudDocs"));
    add("Dropbox".into(), home.join("Dropbox"));
    add("Google Drive".into(), home.join("Google Drive"));
    out
}

/// Takes an encrypted snapshot of the book into `dest`, refusing to leave
/// anything unencrypted behind. This is the only step that needs the book.
fn write_encrypted_snapshot(store: &Store, dest: &Path) -> Result<(), String> {
    let _ = std::fs::remove_file(dest);
    store.backup_to(dest).map_err(|e| e.to_string())?;
    if !is_encrypted_file(dest).unwrap_or(false) {
        let _ = std::fs::remove_file(dest);
        return Err("refusing to copy an unencrypted file".into());
    }
    Ok(())
}

/// Copies the finished snapshot to `dest` through a `.partial` file, so a
/// destination never holds half a book.
fn place_snapshot(snapshot: &Path, dest: &Path) -> Result<(), String> {
    let tmp = dest.with_extension("tallybook.partial");
    let _ = std::fs::remove_file(&tmp);
    std::fs::copy(snapshot, &tmp).map_err(|e| e.to_string())?;
    if !is_encrypted_file(&tmp).unwrap_or(false) {
        let _ = std::fs::remove_file(&tmp);
        return Err("refusing to copy an unencrypted file".into());
    }
    std::fs::rename(&tmp, dest).map_err(|e| e.to_string())
}

/// A scratch folder for the snapshot, removed when dropped.
struct ScratchDir(PathBuf);

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl App {
    pub(crate) async fn cloud(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "cloud.status" => self.with_api(|s| {
                let pref = |k: &str| s.pref(k).ok().flatten();
                let last = pref("cloud.last");
                let auto = pref("cloud.auto").as_deref() == Some("1");
                let stale = last
                    .as_deref()
                    .and_then(|l| DateTime::parse_from_rfc3339(l).ok())
                    .is_none_or(|t| (Local::now() - t.with_timezone(&Local)).num_hours() >= AUTO_EVERY_HOURS);
                let folder = pref("cloud.folder");
                let git = pref("cloud.git");
                Ok(CloudStatus {
                    encrypted: s.is_encrypted(),
                    git_has_remote: git.as_deref().is_some_and(|g| has_remote(Path::new(g))),
                    due: auto && s.is_encrypted() && (folder.is_some() || git.is_some()) && stale,
                    keep: keep_from(pref("cloud.keep")),
                    last_error: pref("cloud.last_error"),
                    folder,
                    git,
                    auto,
                    last,
                    suggestions: suggestions(),
                })
            }),
            "cloud.set" => {
                let p: CloudSet = params(p)?;
                if let Some(Some(f)) = &p.folder {
                    let path = Path::new(f);
                    if !path.is_dir() {
                        return Err(ApiError::new("rule", format!("{f} isn't a folder")));
                    }
                }
                if let Some(Some(g)) = &p.git
                    && !is_git_repo(Path::new(g))
                {
                    return Err(ApiError::new("rule", format!("{g} isn't a Git repository (clone one first, or run `git init`)")));
                }
                self.with(|s| {
                    if let Some(f) = &p.folder {
                        s.set_pref("cloud.folder", f.as_deref())?;
                    }
                    if let Some(g) = &p.git {
                        s.set_pref("cloud.git", g.as_deref())?;
                    }
                    if let Some(a) = p.auto {
                        s.set_pref("cloud.auto", Some(if a { "1" } else { "0" }))?;
                    }
                    if let Some(k) = p.keep {
                        s.set_pref("cloud.keep", Some(&k.clamp(1, MAX_KEEP).to_string()))?;
                    }
                    Ok(())
                })
            }
            "cloud.backup_now" => self.cloud_backup(),
            _ => Err(unknown_method(method)),
        }
    }

    /// `cloud.backup_now`. The book lock is held only to read the settings and
    /// take one encrypted snapshot into a scratch folder; copying it to the
    /// folder, running `git`, and pruning all happen without the lock.
    fn cloud_backup(&self) -> ApiResult<Value> {
        let mut report = BackupReport::default();
        let scratch = ScratchDir(std::env::temp_dir().join(format!("tallybook-cloud-{}", uuid::Uuid::new_v4())));
        let (folder, git_repo, keep, stem, book_name) = {
            let guard = self.lock();
            let book = guard.as_ref().ok_or_else(|| ApiError::new("no_book", "no book is open"))?;
            if !book.store.is_encrypted() {
                return Err(ApiError::new("rule", "Turn on encryption first. Copies that leave this computer are always encrypted."));
            }
            let s = &book.store;
            let stem = book.path.file_stem().and_then(|s| s.to_str()).unwrap_or("book").to_string();
            let name = s.pref("company.name")?.unwrap_or_else(|| stem.clone());
            let keep = keep_from(s.pref("cloud.keep")?);
            let (folder, git_repo) = (s.pref("cloud.folder")?, s.pref("cloud.git")?);
            if folder.is_none() && git_repo.is_none() {
                return Err(ApiError::new("rule", "choose a cloud folder or a Git repository first"));
            }
            std::fs::create_dir_all(&scratch.0).map_err(|e| ApiError::new("io", e.to_string()))?;
            let snapshot = scratch.0.join("snapshot.tallybook");
            write_encrypted_snapshot(s, &snapshot).map_err(|e| ApiError::new("io", e))?;
            (folder, git_repo, keep, stem, name)
        };
        let snapshot = scratch.0.join("snapshot.tallybook");
        let stamp = Local::now().format("%Y-%m-%d %H%M%S").to_string();

        if let Some(folder) = folder {
            report.folder = Some(match copy_to_folder(&snapshot, &folder, &stem, &stamp, keep) {
                Ok(p) => DestResult { ok: true, path: Some(p), detail: None },
                Err(e) => DestResult { ok: false, path: None, detail: Some(e) },
            });
        }

        if let Some(repo) = git_repo {
            report.git = Some(match copy_to_git(&snapshot, &PathBuf::from(repo), &stem, &stamp, &book_name) {
                Ok((p, d)) => DestResult { ok: true, path: Some(p), detail: Some(d) },
                Err(e) => DestResult { ok: false, path: None, detail: Some(e) },
            });
        }

        let failed: Vec<String> = [&report.folder, &report.git].into_iter().flatten().filter(|r| !r.ok).filter_map(|r| r.detail.clone()).collect();
        self.with(|s| {
            s.set_pref("cloud.last", Some(&Local::now().to_rfc3339()))?;
            s.set_pref("cloud.last_error", failed.first().map(String::as_str))
        })?;
        ok(report)
    }
}

/// Writes a dated encrypted copy into "<folder>/Tallybook" and keeps the
/// newest `keep` copies of this book there.
fn copy_to_folder(snapshot: &Path, folder: &str, stem: &str, stamp: &str, keep: usize) -> Result<PathBuf, String> {
    let dir = Path::new(folder).join("Tallybook");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(format!("{stem} {stamp}.tallybook"));
    place_snapshot(snapshot, &dest)?;
    // Keep the newest `keep` copies of this book.
    let prefix = format!("{stem} ");
    let mut copies: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|e| e == "tallybook") && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(&prefix))
        })
        .collect();
    // Never the copy just written, whatever `keep` says.
    copies.retain(|p| *p != dest);
    copies.sort();
    let excess = copies.len().saturating_sub(keep.max(1) - 1);
    for old in copies.into_iter().take(excess) {
        let _ = std::fs::remove_file(old);
    }
    Ok(dest)
}

/// Commits (and pushes, when there is a remote) an encrypted copy into a
/// Git repository. Returns where it landed and what happened.
fn copy_to_git(snapshot: &Path, repo: &Path, stem: &str, stamp: &str, book_name: &str) -> Result<(PathBuf, String), String> {
    if !is_git_repo(repo) {
        return Err(format!("{} is no longer a Git repository", repo.display()));
    }
    let file = format!("{stem}.tallybook");
    let dest = repo.join(&file);
    place_snapshot(snapshot, &dest)?;
    if !is_encrypted_file(&dest).unwrap_or(false) {
        return Err("refusing to commit an unencrypted file".into());
    }
    let readme = repo.join("README.md");
    if !readme.exists() {
        let text = format!(
            "# {book_name} · Tallybook backups\n\nEncrypted copies of a Tallybook book. Each commit is a snapshot.\n\n\
             To restore: open the `.tallybook` file in Tallybook and enter the book's recovery key.\n\
             Without the recovery key these files can't be read.\n"
        );
        std::fs::write(&readme, text).map_err(|e| e.to_string())?;
    }
    git(repo, &["add", "--", &file, "README.md"])?;
    let staged = git(repo, &["diff", "--cached", "--quiet", "--no-ext-diff"]).is_err();
    let mut detail = String::from("No changes since the last copy.");
    if staged {
        git(repo, &["commit", "-m", &format!("Tallybook backup {stamp}"), "--", &file, "README.md"])?;
        detail = "Committed.".into();
    }
    if has_remote(repo) {
        git(repo, &["push"]).map_err(|e| format!("committed locally, but push failed: {e}"))?;
        detail.push_str(" Pushed.");
    } else {
        detail.push_str(" (This repository has no remote, so nothing was pushed.)");
    }
    Ok((dest, detail))
}
