//! SQLite-backed book of accounts.
//!
//! Business rules that need the database live here (accounts exist, lock
//! date, void-by-reversal, reconciliation state). Pure accounting rules live
//! in `tally-core`; the schema's triggers are the last line of defense.
//!
//! A book keeps a business side, a household side, or both (`Store::sides`).
//! Every account sits on one side, and every entry balances within each side
//! on its own: an entry that crosses sides gets owner's-draw lines added when
//! it's posted (`bridge_lines`).

use chrono::{Datelike, NaiveDate};
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;
use std::path::Path;
use tally_core::chart::{self, codes};
use tally_core::journal::EntryId;
use tally_core::report::{self, BalanceSheet, Balances, ProfitAndLoss, TrialBalance};
use tally_core::{Account, AccountId, AccountSubtype, Cents, EntryError, EntryLine, Flex, NewEntry, Scope};

mod budget;
pub mod crypto;
mod feed;
mod insights;
pub mod ledger_import;
mod reconcile;
mod recurring;
mod sales;
mod tax;
pub use budget::*;
pub use feed::*;
pub use insights::*;
pub use reconcile::*;
pub use recurring::*;
pub use sales::*;
pub use tax::*;

pub type LineId = i64;
pub type ReconciliationId = i64;

/// The schema, step by step: (the `PRAGMA user_version` a step brings a book
/// to, its SQL). Each file says in its header what it adds. Steps run in
/// order, each in its own transaction, so a failed upgrade leaves the book at
/// the last version that finished. Steps are only ever appended.
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("schema.sql")),
    (2, include_str!("schema_v2.sql")),  // bank feeds
    (3, include_str!("schema_v3.sql")),  // customers, vendors, invoices, bills
    (4, include_str!("schema_v4.sql")),  // tax center
    (5, include_str!("schema_v5.sql")),  // household side: account scope, flex
    (6, include_str!("schema_v6.sql")),  // people and monthly budgets
    (7, include_str!("schema_v7.sql")),  // recurring household bills
    (8, include_str!("schema_v8.sql")),  // savings goals
    (9, include_str!("schema_v9.sql")),  // import_links, for re-importing
    (10, include_str!("schema_v10.sql")), // bills whose amount varies
    (11, include_str!("schema_v11.sql")), // matching register lines by account and amount
    (12, include_str!("schema_v12.sql")), // application_id: the file says it's a Tallybook book
];

/// The newest schema this build knows: the last step of `MIGRATIONS`.
const SCHEMA_VERSION: i64 = 12;

/// `PRAGMA application_id` of every book from schema v12 on: the ASCII
/// letters "TALY" read as a big-endian 32-bit number. It sits in bytes 68-71
/// of the SQLite header so any tool can tell a Tallybook book from some other
/// SQLite file. Set by `schema_v12.sql`; never change it.
pub const APPLICATION_ID: i32 = 0x5441_4C59;
const _: () = assert!(MIGRATIONS[MIGRATIONS.len() - 1].0 == SCHEMA_VERSION);

/// Tables every Tallybook book has had since schema v1. A file with tables but
/// not these is some other program's database.
const BOOK_TABLES: [&str; 3] = ["accounts", "journal_entries", "settings"];

/// Settings for every connection to a book, since a book may come from
/// someone else: SQLite's defensive mode, and no functions with side effects
/// run from the file's own schema (views, triggers).
pub(crate) fn harden(conn: &Connection) -> Result<()> {
    conn.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
    conn.pragma_update(None, "trusted_schema", false)?;
    Ok(())
}

/// Names of the triggers and views Tallybook's migrations create.
fn schema_object_names() -> Vec<String> {
    let mut names = Vec::new();
    for (_, sql) in MIGRATIONS {
        let words: Vec<&str> = sql.split_whitespace().collect();
        for (i, w) in words.iter().enumerate() {
            if !w.eq_ignore_ascii_case("CREATE") {
                continue;
            }
            let rest = words[i + 1..].iter().skip_while(|w| w.eq_ignore_ascii_case("TEMP") || w.eq_ignore_ascii_case("TEMPORARY"));
            let mut rest = rest.peekable();
            if rest.next_if(|w| w.eq_ignore_ascii_case("TRIGGER") || w.eq_ignore_ascii_case("VIEW")).is_none() {
                continue;
            }
            if let Some(name) = rest.find(|w| !["IF", "NOT", "EXISTS"].iter().any(|k| w.eq_ignore_ascii_case(k))) {
                names.push(name.trim_matches(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).to_string());
            }
        }
    }
    names
}

/// Refuses, before anything writes to it, a file that isn't a Tallybook book:
/// another program's database (by its application id, or for files from
/// before v12 by its tables), or one holding triggers or views Tallybook
/// didn't create. A new, empty file passes.
pub(crate) fn check_is_book(conn: &Connection) -> Result<()> {
    let application_id: i32 = conn.pragma_query_value(None, "application_id", |r| r.get(0))?;
    let not_a_book = || rule("this file is another program's database, not a Tallybook book");
    if application_id != 0 && application_id != APPLICATION_ID {
        return not_a_book();
    }
    let mut stmt = conn.prepare("SELECT type, name FROM sqlite_master WHERE type IN ('table', 'view', 'trigger') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'")?;
    let objects: Vec<(String, String)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    let has = |t: &str| objects.iter().any(|(ty, n)| ty == "table" && n == t);
    if application_id == 0 && objects.iter().any(|(ty, _)| ty == "table") && !BOOK_TABLES.iter().all(|t| has(t)) {
        return not_a_book();
    }
    let known = schema_object_names();
    if let Some((ty, name)) = objects.iter().find(|(ty, n)| ty != "table" && !known.contains(n)) {
        return rule(format!("this book contains a {ty} ({name}) that Tallybook didn't create, so it won't be opened"));
    }
    Ok(())
}

/// Settings key for the sides this book keeps: `Scope` names joined by
/// commas, business first, e.g. `business,household`. Missing means a book
/// from before household support, which is business-only.
const SIDES_KEY: &str = "book.sides";

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Entry(#[from] EntryError),
    #[error("{0} {1} not found")]
    NotFound(&'static str, i64),
    /// The book is encrypted and was opened without its key.
    #[error("this book is encrypted")]
    Locked,
    /// A bookkeeping rule was violated; the message is safe to show users.
    #[error("{0}")]
    Rule(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

fn rule<T>(msg: impl Into<String>) -> Result<T> {
    Err(StoreError::Rule(msg.into()))
}

// ---------------------------------------------------------------- types

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAccount {
    pub code: Option<String>,
    pub name: String,
    pub subtype: AccountSubtype,
    pub parent_id: Option<AccountId>,
    pub scope: Scope,
    pub flex: Option<Flex>,
}

impl NewAccount {
    /// A business account with no code or parent.
    pub fn new(name: impl Into<String>, subtype: AccountSubtype) -> NewAccount {
        NewAccount { code: None, name: name.into(), subtype, parent_id: None, scope: Scope::Business, flex: None }
    }

    /// Gives it a chart code ("6100", "H6200").
    pub fn code(mut self, code: impl Into<String>) -> NewAccount {
        self.code = Some(code.into());
        self
    }

    /// Puts it on the given side of the book.
    pub fn scope(mut self, scope: Scope) -> NewAccount {
        self.scope = scope;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Cleared {
    Uncleared,
    Cleared,
    Reconciled,
}

impl Cleared {
    fn as_str(self) -> &'static str {
        match self {
            Cleared::Uncleared => "uncleared",
            Cleared::Cleared => "cleared",
            Cleared::Reconciled => "reconciled",
        }
    }

    fn parse(s: &str) -> Option<Cleared> {
        match s {
            "uncleared" => Some(Cleared::Uncleared),
            "cleared" => Some(Cleared::Cleared),
            "reconciled" => Some(Cleared::Reconciled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PostedLine {
    pub id: LineId,
    pub account_id: AccountId,
    /// Raw: positive debit, negative credit.
    pub amount: Cents,
    pub memo: Option<String>,
    pub cleared: Cleared,
    pub reconciliation_id: Option<ReconciliationId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub id: EntryId,
    pub date: NaiveDate,
    pub memo: String,
    pub source: String,
    pub reverses_id: Option<EntryId>,
    pub voided_by_id: Option<EntryId>,
    /// Customer or vendor this entry was with, if known.
    pub payee_id: Option<i64>,
    /// Who the money was for (household books).
    pub person_id: Option<PersonId>,
    pub lines: Vec<PostedLine>,
}

/// One line of an account register, in the account's natural sign.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RegisterRow {
    pub entry_id: EntryId,
    pub line_id: LineId,
    pub date: NaiveDate,
    pub memo: String,
    pub amount: Cents,
    pub balance: Cents,
    pub cleared: Cleared,
    pub voided: bool,
}

// ---------------------------------------------------------------- row mapping

fn conversion_err(col: usize, e: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(col, Type::Text, Box::new(e))
}

#[derive(Debug, thiserror::Error)]
#[error("bad stored value: {0:?}")]
struct BadValue(String);

const ACCOUNT_COLS: &str = "id, code, name, subtype, parent_id, archived, scope, flex";

fn account_from_row(row: &Row) -> rusqlite::Result<Account> {
    let subtype: String = row.get(3)?;
    Ok(Account {
        id: row.get(0)?,
        code: row.get(1)?,
        name: row.get(2)?,
        subtype: subtype.parse().map_err(|e| conversion_err(3, e))?,
        parent_id: row.get(4)?,
        archived: row.get(5)?,
        scope: row.get::<_, String>(6)?.parse().map_err(|e| conversion_err(6, e))?,
        flex: row.get::<_, Option<String>>(7)?.map(|f| f.parse()).transpose().map_err(|e| conversion_err(7, e))?,
    })
}

fn cleared_from(col: usize, s: String) -> rusqlite::Result<Cleared> {
    Cleared::parse(&s).ok_or_else(|| conversion_err(col, BadValue(s)))
}

// ---------------------------------------------------------------- settings

fn setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
        .optional()?)
}

fn set_setting(conn: &Connection, key: &str, value: Option<&str>) -> Result<()> {
    match value {
        Some(v) => conn.execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, v],
        )?,
        None => conn.execute("DELETE FROM settings WHERE key = ?1", [key])?,
    };
    Ok(())
}

fn lock_date_in(conn: &Connection) -> Result<Option<NaiveDate>> {
    match setting(conn, "lock_date")? {
        None => Ok(None),
        Some(s) => s
            .parse()
            .map(Some)
            .map_err(|_| StoreError::Rule(format!("corrupt lock_date setting: {s:?}"))),
    }
}

fn check_not_locked(conn: &Connection, date: NaiveDate) -> Result<()> {
    if let Some(lock) = lock_date_in(conn)?
        && date <= lock
    {
        return rule(format!("{date} is on or before the books' lock date ({lock})"));
    }
    Ok(())
}

/// [`check_not_locked`] for an existing entry, by its date. `NotFound` when
/// there is no such entry.
fn check_entry_not_locked(conn: &Connection, entry_id: EntryId) -> Result<()> {
    let date: Option<NaiveDate> =
        conn.query_row("SELECT date FROM journal_entries WHERE id = ?1", [entry_id], |r| r.get(0)).optional()?;
    check_not_locked(conn, date.ok_or(StoreError::NotFound("entry", entry_id))?)
}

// ---------------------------------------------------------------- store

pub struct Store {
    conn: Connection,
    /// Set for encrypted books; also encrypts every backup made from them.
    key: Option<crypto::BookKey>,
}

impl Store {
    /// Opens (or creates) an unencrypted book. Encrypted books need
    /// [`Store::open_encrypted`].
    pub fn open(path: impl AsRef<Path>) -> Result<Store> {
        let path = path.as_ref();
        if crypto::is_encrypted_file(path).unwrap_or(false) {
            return Err(StoreError::Locked);
        }
        let conn = Connection::open(path)?;
        harden(&conn)?;
        // Before WAL mode or migrations write anything to the file.
        check_is_book(&conn)?;
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        // The desktop app and an agent (tally-mcp) may have the same book open;
        // wait out the other's write instead of failing with "database is locked".
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Store::init(conn, None)
    }

    pub fn open_in_memory() -> Result<Store> {
        let conn = Connection::open_in_memory()?;
        harden(&conn)?;
        Store::init(conn, None)
    }

    fn init(conn: Connection, key: Option<crypto::BookKey>) -> Result<Store> {
        conn.pragma_update(None, "foreign_keys", true)?;
        let mut store = Store { conn, key };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&mut self) -> Result<()> {
        // 0 is SQLite's default: a new file, or a book from before v12.
        // Anything else is another program's database, and running our
        // migrations would write Tallybook tables into it.
        let application_id: i32 = self.conn.pragma_query_value(None, "application_id", |r| r.get(0))?;
        if application_id != 0 && application_id != APPLICATION_ID {
            return rule(format!(
                "this file is another program's database (SQLite application id {application_id:#010x}), not a Tallybook book"
            ));
        }
        let version: i64 = self.conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return rule(format!(
                "this book was created by a newer Tallybook (schema {version}); please upgrade"
            ));
        }
        for &(step, sql) in MIGRATIONS {
            if version < step {
                let tx = self.conn.savepoint()?;
                tx.execute_batch(sql)?;
                tx.pragma_update(None, "user_version", step)?;
                tx.commit()?;
            }
        }
        Ok(())
    }

    /// Writes a consistent, compacted copy of the whole book to `path`.
    /// Copies of an encrypted book are encrypted with the same key.
    pub fn backup_to(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        if path.exists() {
            return rule(format!("{} already exists", path.display()));
        }
        match &self.key {
            Some(key) => self.export_encrypted(path, key),
            None => {
                self.conn.execute("VACUUM INTO ?1", [path.to_string_lossy()])?;
                Ok(())
            }
        }
    }

    /// Runs `f` as one unit: every change it makes is kept, or, if it fails
    /// (or panics), none is. For operations made of several steps, such as
    /// creating an account with its opening balance. The steps' own
    /// transactions nest inside it as savepoints.
    pub fn atomically<T>(&mut self, f: impl FnOnce(&mut Store) -> Result<T>) -> Result<T> {
        self.conn.execute_batch("SAVEPOINT atomically")?;
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(self)));
        match outcome {
            Ok(Ok(v)) => {
                self.conn.execute_batch("RELEASE atomically")?;
                Ok(v)
            }
            Ok(Err(e)) => {
                self.conn.execute_batch("ROLLBACK TO atomically; RELEASE atomically")?;
                Err(e)
            }
            Err(panic) => {
                let _ = self.conn.execute_batch("ROLLBACK TO atomically; RELEASE atomically");
                std::panic::resume_unwind(panic)
            }
        }
    }

    /// Raw SQL, for tests that prove the schema's triggers hold without the API.
    #[cfg(feature = "testing")]
    pub fn debug_execute(&self, sql: &str) -> rusqlite::Result<usize> {
        self.conn.execute(sql, [])
    }

    // ------------------------------------------------------------ settings

    pub fn lock_date(&self) -> Result<Option<NaiveDate>> {
        lock_date_in(&self.conn)
    }

    /// Entries dated on or before the lock date can't be posted or voided.
    pub fn set_lock_date(&self, date: Option<NaiveDate>) -> Result<()> {
        set_setting(&self.conn, "lock_date", date.map(|d| d.to_string()).as_deref())
    }

    /// 1 = January. Drives where the balance sheet splits retained earnings from net income.
    pub fn fiscal_year_start_month(&self) -> Result<u32> {
        match setting(&self.conn, "fiscal_year_start_month")? {
            None => Ok(1),
            Some(s) => s
                .parse()
                .ok()
                .filter(|m| (1..=12).contains(m))
                .ok_or_else(|| StoreError::Rule(format!("corrupt fiscal_year_start_month: {s:?}"))),
        }
    }

    pub fn set_fiscal_year_start_month(&self, month: u32) -> Result<()> {
        if !(1..=12).contains(&month) {
            return rule("fiscal year start month must be 1-12");
        }
        set_setting(&self.conn, "fiscal_year_start_month", Some(&month.to_string()))
    }

    // ------------------------------------------------------------ accounts

    pub fn create_account(&self, new: &NewAccount) -> Result<AccountId> {
        let name = new.name.trim();
        if name.is_empty() {
            return rule("account name is required");
        }
        let code = new.code.as_deref().map(str::trim).filter(|c| !c.is_empty());
        if let Some(parent_id) = new.parent_id {
            let parent = self.account(parent_id)?;
            if parent.kind() != new.subtype.kind() {
                return rule(format!("a sub-account must have the same type as its parent ({})", parent.name));
            }
            if parent.scope != new.scope {
                return rule(format!("a sub-account must be on the same side of the book as its parent ({})", parent.name));
            }
        }
        if !self.sides()?.contains(&new.scope) {
            return rule(format!("this book has no {} side yet; add one in Settings", new.scope.as_str()));
        }
        if new.flex.is_some() && !(new.scope == Scope::Household && new.subtype.kind() == tally_core::AccountKind::Expense) {
            return rule("only household spending categories have a flexibility");
        }
        if let Some(code) = code
            && self.account_by_code(code)?.is_some()
        {
            return rule(format!("account code {code} is already in use"));
        }
        self.conn.execute(
            "INSERT INTO accounts(code, name, subtype, parent_id, scope, flex) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![code, name, new.subtype.as_str(), new.parent_id, new.scope.as_str(), new.flex.map(Flex::as_str)],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Adds the business starter chart, skipping codes that already exist.
    pub fn seed_default_chart(&mut self) -> Result<()> {
        self.add_side(Scope::Business)
    }

    /// The sides this book keeps, business first. Books from before
    /// household support are business-only.
    pub fn sides(&self) -> Result<Vec<Scope>> {
        let Some(raw) = setting(&self.conn, SIDES_KEY)? else {
            return Ok(vec![Scope::Business]);
        };
        let mut sides = raw
            .split(',')
            .map(|s| s.parse::<Scope>().map_err(|_| StoreError::Rule(format!("corrupt {SIDES_KEY} setting: {raw:?}"))))
            .collect::<Result<Vec<_>>>()?;
        sides.sort_by_key(|s| *s as u8);
        sides.dedup();
        Ok(sides)
    }

    /// Starts keeping one side of the book: seeds its starter chart (skipping
    /// codes that already exist) and records the side. A brand-new book that
    /// gets only a household side becomes household-only.
    pub fn add_side(&mut self, scope: Scope) -> Result<()> {
        let fresh = setting(&self.conn, SIDES_KEY)?.is_none()
            && self.conn.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get::<_, i64>(0))? == 0;
        let mut sides = if fresh { vec![] } else { self.sides()? };
        if !sides.contains(&scope) {
            sides.push(scope);
        }
        sides.sort_by_key(|s| *s as u8);
        let tx = self.conn.savepoint()?;
        for t in chart::chart_for(scope) {
            tx.execute(
                "INSERT INTO accounts(code, name, subtype, scope, flex) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(code) DO NOTHING",
                params![t.code, t.name, t.subtype.as_str(), scope.as_str(), t.flex.map(Flex::as_str)],
            )?;
        }
        let joined = sides.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(",");
        set_setting(&tx, SIDES_KEY, Some(&joined))?;
        tx.commit()?;
        Ok(())
    }

    /// Accounts on one side of the book, or all of them.
    pub fn accounts_in(&self, scope: Option<Scope>) -> Result<Vec<Account>> {
        let mut all = self.accounts()?;
        if let Some(scope) = scope {
            all.retain(|a| a.scope == scope);
        }
        Ok(all)
    }

    /// How far a household spending category can bend (fixed, usage,
    /// lifestyle); `None` clears it. Only those categories carry one.
    pub fn set_flex(&self, id: AccountId, flex: Option<Flex>) -> Result<()> {
        let account = self.account(id)?;
        if flex.is_some() && !(account.scope == Scope::Household && account.kind() == tally_core::AccountKind::Expense) {
            return rule("only household spending categories have a flexibility");
        }
        self.conn.execute("UPDATE accounts SET flex = ?1 WHERE id = ?2", params![flex.map(Flex::as_str), id])?;
        Ok(())
    }

    pub fn accounts(&self) -> Result<Vec<Account>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {ACCOUNT_COLS} FROM accounts ORDER BY code IS NULL, code, name"
        ))?;
        let rows = stmt.query_map([], account_from_row)?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn account(&self, id: AccountId) -> Result<Account> {
        self.conn
            .query_row(&format!("SELECT {ACCOUNT_COLS} FROM accounts WHERE id = ?1"), [id], account_from_row)
            .optional()?
            .ok_or(StoreError::NotFound("account", id))
    }

    pub fn account_by_code(&self, code: &str) -> Result<Option<Account>> {
        Ok(self
            .conn
            .query_row(&format!("SELECT {ACCOUNT_COLS} FROM accounts WHERE code = ?1"), [code], account_from_row)
            .optional()?)
    }

    pub fn rename_account(&self, id: AccountId, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return rule("account name is required");
        }
        if self.conn.execute("UPDATE accounts SET name = ?1 WHERE id = ?2", params![name, id])? == 0 {
            return Err(StoreError::NotFound("account", id));
        }
        Ok(())
    }

    /// Archived accounts keep their history but accept no new entries.
    pub fn set_archived(&self, id: AccountId, archived: bool) -> Result<()> {
        let account = self.account(id)?;
        if archived {
            let balance: i64 = self.conn.query_row(
                "SELECT IFNULL(SUM(amount), 0) FROM journal_lines WHERE account_id = ?1",
                [id],
                |r| r.get(0),
            )?;
            if balance != 0 {
                return rule(format!("{} still has a balance; move it before archiving", account.name));
            }
        }
        self.conn.execute("UPDATE accounts SET archived = ?1 WHERE id = ?2", params![archived, id])?;
        Ok(())
    }

    // ------------------------------------------------------------ journal

    pub fn post_entry(&mut self, entry: &NewEntry) -> Result<EntryId> {
        self.post_entry_from(entry, "manual")
    }

    /// `source` records where the entry came from (`manual`, `bank_feed`, `invoice`, ...).
    pub fn post_entry_from(&mut self, entry: &NewEntry, source: &str) -> Result<EntryId> {
        let tx = self.conn.savepoint()?;
        let id = post_in(&tx, entry, source, None)?;
        tx.commit()?;
        Ok(id)
    }

    pub fn entry(&self, id: EntryId) -> Result<Entry> {
        entry_in(&self.conn, id)
    }

    /// Entries dated within `[from, to]`, oldest first.
    pub fn entries(&self, from: NaiveDate, to: NaiveDate) -> Result<Vec<Entry>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM journal_entries WHERE date BETWEEN ?1 AND ?2 ORDER BY date, id")?;
        let ids: Vec<EntryId> = stmt.query_map(params![from, to], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        ids.into_iter().map(|id| entry_in(&self.conn, id)).collect()
    }

    /// Cancels an entry by posting its exact reversal on `date`. History is kept.
    pub fn void_entry(&mut self, id: EntryId, date: NaiveDate) -> Result<EntryId> {
        let tx = self.conn.savepoint()?;
        let original = entry_in(&tx, id)?;
        if original.voided_by_id.is_some() {
            return rule(format!("entry #{id} is already voided"));
        }
        if original.reverses_id.is_some() {
            return rule(format!("entry #{id} is itself a void and can't be voided"));
        }
        if original.lines.iter().any(|l| l.cleared == Cleared::Reconciled) {
            return rule(format!("entry #{id} is part of a completed reconciliation; undo that first"));
        }
        if date < original.date {
            return rule("a void can't be dated before the entry it cancels");
        }

        let as_new = NewEntry {
            date: original.date,
            memo: original.memo.clone(),
            lines: original
                .lines
                .iter()
                .map(|l| EntryLine { account_id: l.account_id, amount: l.amount, memo: l.memo.clone() })
                .collect(),
        };
        let reversal = as_new.reversal(date, format!("Void of #{id}: {}", original.memo));
        let reversal_id = post_in(&tx, &reversal, "void", Some(id))?;
        tx.execute("UPDATE journal_entries SET voided_by_id = ?1 WHERE id = ?2", params![reversal_id, id])?;

        // Give each reversal line the same cleared state as the line it
        // cancels, so a voided pair always nets to zero in a reconciliation.
        let reversed = entry_in(&tx, reversal_id)?;
        for (orig, rev) in original.lines.iter().zip(&reversed.lines) {
            tx.execute(
                "UPDATE journal_lines SET cleared = ?1, reconciliation_id = ?2 WHERE id = ?3",
                params![orig.cleared.as_str(), orig.reconciliation_id, rev.id],
            )?;
        }
        tx.commit()?;
        Ok(reversal_id)
    }

    /// Every line touching `account_id`, oldest first, with a running balance.
    pub fn register(&self, account_id: AccountId) -> Result<Vec<RegisterRow>> {
        let kind = self.account(account_id)?.kind();
        let mut stmt = self.conn.prepare(
            "SELECT e.id, l.id, e.date, COALESCE(l.memo, e.memo), l.amount, l.cleared,
                    e.voided_by_id IS NOT NULL OR e.reverses_id IS NOT NULL
             FROM journal_lines l JOIN journal_entries e ON e.id = l.entry_id
             WHERE l.account_id = ?1
             ORDER BY e.date, e.id, l.id",
        )?;
        let mut balance = Cents::ZERO;
        let rows = stmt
            .query_map([account_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, Cents(r.get(4)?), cleared_from(5, r.get(5)?)?, r.get(6)?))
            })?
            .map(|row| {
                let (entry_id, line_id, date, memo, raw, cleared, voided) = row?;
                let amount = kind.natural(raw);
                balance += amount;
                Ok(RegisterRow { entry_id, line_id, date, memo, amount, balance, cleared, voided })
            })
            .collect::<Result<_>>()?;
        Ok(rows)
    }

    // ------------------------------------------------------------ balances & reports

    /// Raw balances for entries dated within `[from, to]` (`from = None` means since the beginning).
    pub fn balances(&self, from: Option<NaiveDate>, to: NaiveDate) -> Result<Balances> {
        let mut stmt = self.conn.prepare(
            "SELECT l.account_id, SUM(l.amount)
             FROM journal_lines l JOIN journal_entries e ON e.id = l.entry_id
             WHERE (?1 IS NULL OR e.date >= ?1) AND e.date <= ?2
             GROUP BY l.account_id",
        )?;
        let balances = stmt
            .query_map(params![from, to], |r| Ok((r.get(0)?, Cents(r.get(1)?))))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(balances)
    }

    /// Natural-sign balance of one account through `as_of`.
    pub fn account_balance(&self, id: AccountId, as_of: NaiveDate) -> Result<Cents> {
        let kind = self.account(id)?.kind();
        let raw: i64 = self.conn.query_row(
            "SELECT IFNULL(SUM(l.amount), 0)
             FROM journal_lines l JOIN journal_entries e ON e.id = l.entry_id
             WHERE l.account_id = ?1 AND e.date <= ?2",
            params![id, as_of],
            |r| r.get(0),
        )?;
        Ok(kind.natural(Cents(raw)))
    }

    pub fn trial_balance(&self, as_of: NaiveDate) -> Result<TrialBalance> {
        self.trial_balance_in(as_of, None)
    }

    /// Each side of the book balances on its own, so a one-side trial balance does too.
    pub fn trial_balance_in(&self, as_of: NaiveDate, scope: Option<Scope>) -> Result<TrialBalance> {
        Ok(report::trial_balance(&self.accounts_in(scope)?, &self.balances(None, as_of)?))
    }

    pub fn profit_and_loss(&self, from: NaiveDate, to: NaiveDate) -> Result<ProfitAndLoss> {
        self.profit_and_loss_in(from, to, None)
    }

    /// Profit and loss for one side of the book, or the whole book with `None`.
    pub fn profit_and_loss_in(&self, from: NaiveDate, to: NaiveDate, scope: Option<Scope>) -> Result<ProfitAndLoss> {
        Ok(report::profit_and_loss(&self.accounts_in(scope)?, &self.balances(Some(from), to)?))
    }

    pub fn fiscal_year_start(&self, date: NaiveDate) -> Result<NaiveDate> {
        let month = self.fiscal_year_start_month()?;
        let year = if date.month() >= month { date.year() } else { date.year() - 1 };
        NaiveDate::from_ymd_opt(year, month, 1).ok_or_else(|| StoreError::Rule("date out of range".into()))
    }

    pub fn balance_sheet(&self, as_of: NaiveDate) -> Result<BalanceSheet> {
        self.balance_sheet_in(as_of, None)
    }

    /// The balance sheet for one side of the book, or the whole book with `None`.
    pub fn balance_sheet_in(&self, as_of: NaiveDate, scope: Option<Scope>) -> Result<BalanceSheet> {
        let fy_start = self.fiscal_year_start(as_of)?;
        let before = match fy_start.pred_opt() {
            Some(day_before) => self.balances(None, day_before)?,
            None => Balances::new(),
        };
        Ok(report::balance_sheet(&self.accounts_in(scope)?, &self.balances(None, as_of)?, &before))
    }
}

// ---------------------------------------------------------------- shared helpers

/// Writes a validated entry inside `tx`, the one place every posting goes
/// through. Its lines must name live accounts; when they cross sides of the
/// book, `bridge_lines` are added so each side still balances on its own.
fn post_in(tx: &Connection, entry: &NewEntry, source: &str, reverses: Option<EntryId>) -> Result<EntryId> {
    entry.validate()?;
    check_not_locked(tx, entry.date)?;
    // Sum of the business lines; checked, since imported amounts can be huge.
    let mut business = Cents::ZERO;
    for line in &entry.lines {
        let found: Option<(String, bool, String)> = tx
            .query_row("SELECT name, archived, scope FROM accounts WHERE id = ?1", [line.account_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?;
        match found {
            None => return Err(StoreError::NotFound("account", line.account_id)),
            Some((name, true, _)) => return rule(format!("{name} is archived")),
            Some((_, false, scope)) => {
                if scope == Scope::Business.as_str() {
                    business = business.checked_add(line.amount).ok_or(EntryError::Overflow)?;
                }
            }
        }
    }
    let bridge = bridge_lines(tx, business)?;
    tx.execute(
        "INSERT INTO journal_entries(date, memo, source, reverses_id) VALUES (?1, ?2, ?3, ?4)",
        params![entry.date, entry.memo.trim(), source, reverses],
    )?;
    let id = tx.last_insert_rowid();
    let mut insert =
        tx.prepare_cached("INSERT INTO journal_lines(entry_id, account_id, amount, memo) VALUES (?1, ?2, ?3, ?4)")?;
    for line in entry.lines.iter().chain(&bridge) {
        insert.execute(params![id, line.account_id, line.amount.0, line.memo])?;
    }
    Ok(id)
}

/// Lines that make an entry balance within each side of the book.
///
/// `business` is the sum of the entry's business lines. Since the whole entry
/// sums to zero, the household lines sum to its negation. A business side
/// that is credit-heavy paid for something at home: an owner's draw, which
/// the household sees as money from the business. Debit-heavy is the reverse,
/// an owner's contribution.
fn bridge_lines(tx: &Connection, business: Cents) -> Result<Vec<EntryLine>> {
    if business.is_zero() {
        return Ok(vec![]);
    }
    let find = |code: &str| -> Result<AccountId> {
        let found: Option<(AccountId, bool)> = tx
            .query_row("SELECT id, archived FROM accounts WHERE code = ?1", [code], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        match found {
            Some((id, false)) => Ok(id),
            _ => rule(format!(
                "this mixes business and household money, but account {code} is missing or archived, so the owner's draw can't be recorded"
            )),
        }
    };
    let amount = business.abs();
    Ok(if business.0 < 0 {
        vec![
            EntryLine::debit(find(codes::OWNERS_DRAW)?, amount).with_memo("Owner's draw"),
            EntryLine::credit(find(codes::FROM_THE_BUSINESS)?, amount).with_memo("Owner's draw"),
        ]
    } else {
        vec![
            EntryLine::credit(find(codes::OWNERS_EQUITY)?, amount).with_memo("Owner's contribution"),
            EntryLine::debit(find(codes::INTO_THE_BUSINESS)?, amount).with_memo("Owner's contribution"),
        ]
    })
}

fn entry_in(conn: &Connection, id: EntryId) -> Result<Entry> {
    let head = conn
        .query_row(
            "SELECT date, memo, source, reverses_id, voided_by_id, payee_id, person_id FROM journal_entries WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )
        .optional()?
        .ok_or(StoreError::NotFound("entry", id))?;
    let mut stmt = conn.prepare_cached(
        "SELECT id, account_id, amount, memo, cleared, reconciliation_id
         FROM journal_lines WHERE entry_id = ?1 ORDER BY id",
    )?;
    let lines = stmt
        .query_map([id], |r| {
            Ok(PostedLine {
                id: r.get(0)?,
                account_id: r.get(1)?,
                amount: Cents(r.get(2)?),
                memo: r.get(3)?,
                cleared: cleared_from(4, r.get(4)?)?,
                reconciliation_id: r.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let (date, memo, source, reverses_id, voided_by_id, payee_id, person_id) = head;
    Ok(Entry { id, date, memo, source, reverses_id, voided_by_id, payee_id, person_id, lines })
}

impl Store {

    /// Every `pref.*` setting, without the prefix.
    pub fn prefs(&self) -> Result<std::collections::BTreeMap<String, String>> {
        let mut stmt = self.conn.prepare("SELECT substr(key, 6), value FROM settings WHERE key LIKE 'pref.%'")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }
}
