//! SQLite-backed book of accounts.
//!
//! Business rules that need the database live here (accounts exist, lock
//! date, void-by-reversal, reconciliation state). Pure accounting rules live
//! in `tally-core`; the schema's triggers are the last line of defense.

use chrono::{Datelike, NaiveDate};
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use serde::Serialize;
use std::path::Path;
use tally_core::chart::{self, codes};
use tally_core::journal::EntryId;
use tally_core::reconcile::ReconcileSummary;
use tally_core::report::{self, BalanceSheet, Balances, ProfitAndLoss, TrialBalance};
use tally_core::{Account, AccountId, AccountSubtype, Cents, EntryError, EntryLine, Flex, NewEntry, Scope};

mod budget;
pub mod crypto;
mod feed;
mod insights;
mod recurring;
mod sales;
mod tax;
pub use budget::*;
pub use feed::*;
pub use insights::*;
pub use recurring::*;
pub use sales::*;
pub use tax::*;

pub type LineId = i64;
pub type ReconciliationId = i64;

const SCHEMA_VERSION: i64 = 8;

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

    pub fn code(mut self, code: impl Into<String>) -> NewAccount {
        self.code = Some(code.into());
        self
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconciliationStatus {
    InProgress,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Reconciliation {
    pub id: ReconciliationId,
    pub account_id: AccountId,
    pub statement_date: NaiveDate,
    /// Natural sign (credit card: amount owed).
    pub statement_balance: Cents,
    pub status: ReconciliationStatus,
}

/// A line the user can tick off against a statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReconcileCandidate {
    pub line_id: LineId,
    pub entry_id: EntryId,
    pub date: NaiveDate,
    pub memo: String,
    /// Natural sign.
    pub amount: Cents,
    pub checked: bool,
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
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        Store::init(conn, None)
    }

    pub fn open_in_memory() -> Result<Store> {
        Store::init(Connection::open_in_memory()?, None)
    }

    fn init(conn: Connection, key: Option<crypto::BookKey>) -> Result<Store> {
        conn.pragma_update(None, "foreign_keys", true)?;
        let mut store = Store { conn, key };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&mut self) -> Result<()> {
        let version: i64 = self.conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return rule(format!(
                "this book was created by a newer Tallybook (schema {version}); please upgrade"
            ));
        }
        if version < 1 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(include_str!("schema.sql"))?;
            tx.pragma_update(None, "user_version", 1)?;
            tx.commit()?;
        }
        if version < 2 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(include_str!("schema_v2.sql"))?;
            tx.pragma_update(None, "user_version", 2)?;
            tx.commit()?;
        }
        if version < 3 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(include_str!("schema_v3.sql"))?;
            tx.pragma_update(None, "user_version", 3)?;
            tx.commit()?;
        }
        if version < 4 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(include_str!("schema_v4.sql"))?;
            tx.pragma_update(None, "user_version", 4)?;
            tx.commit()?;
        }
        if version < 5 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(include_str!("schema_v5.sql"))?;
            tx.pragma_update(None, "user_version", 5)?;
            tx.commit()?;
        }
        if version < 6 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(include_str!("schema_v6.sql"))?;
            tx.pragma_update(None, "user_version", 6)?;
            tx.commit()?;
        }
        if version < 7 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(include_str!("schema_v7.sql"))?;
            tx.pragma_update(None, "user_version", 7)?;
            tx.commit()?;
        }
        if version < 8 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(include_str!("schema_v8.sql"))?;
            tx.pragma_update(None, "user_version", 8)?;
            tx.commit()?;
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
        let Some(raw) = setting(&self.conn, "book.sides")? else {
            return Ok(vec![Scope::Business]);
        };
        let mut sides = raw
            .split(',')
            .map(|s| s.parse::<Scope>().map_err(|_| StoreError::Rule(format!("corrupt book.sides setting: {raw:?}"))))
            .collect::<Result<Vec<_>>>()?;
        sides.sort_by_key(|s| *s as u8);
        sides.dedup();
        Ok(sides)
    }

    /// Starts keeping one side of the book: seeds its starter chart (skipping
    /// codes that already exist) and records the side. A brand-new book that
    /// gets only a household side becomes household-only.
    pub fn add_side(&mut self, scope: Scope) -> Result<()> {
        let fresh = setting(&self.conn, "book.sides")?.is_none()
            && self.conn.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get::<_, i64>(0))? == 0;
        let mut sides = if fresh { vec![] } else { self.sides()? };
        if !sides.contains(&scope) {
            sides.push(scope);
        }
        sides.sort_by_key(|s| *s as u8);
        let tx = self.conn.transaction()?;
        for t in chart::chart_for(scope) {
            tx.execute(
                "INSERT INTO accounts(code, name, subtype, scope, flex) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(code) DO NOTHING",
                params![t.code, t.name, t.subtype.as_str(), scope.as_str(), t.flex.map(Flex::as_str)],
            )?;
        }
        let joined = sides.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(",");
        set_setting(&tx, "book.sides", Some(&joined))?;
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
        let tx = self.conn.transaction()?;
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
        let tx = self.conn.transaction()?;
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

    pub fn balance_sheet_in(&self, as_of: NaiveDate, scope: Option<Scope>) -> Result<BalanceSheet> {
        let fy_start = self.fiscal_year_start(as_of)?;
        let before = match fy_start.pred_opt() {
            Some(day_before) => self.balances(None, day_before)?,
            None => Balances::new(),
        };
        Ok(report::balance_sheet(&self.accounts_in(scope)?, &self.balances(None, as_of)?, &before))
    }

    // ------------------------------------------------------------ reconciliation

    /// Begins reconciling `account_id` against a statement. `statement_balance`
    /// is in the account's natural sign. Lines already marked cleared (e.g. by
    /// a bank-feed match) and dated on or before the statement are pre-checked.
    pub fn start_reconciliation(
        &mut self,
        account_id: AccountId,
        statement_date: NaiveDate,
        statement_balance: Cents,
    ) -> Result<ReconciliationId> {
        let account = self.account(account_id)?;
        if !account.kind().is_balance_sheet() {
            return rule("only balance-sheet accounts can be reconciled");
        }
        let tx = self.conn.transaction()?;
        let open: Option<ReconciliationId> = tx
            .query_row(
                "SELECT id FROM reconciliations WHERE account_id = ?1 AND status = 'in_progress'",
                [account_id],
                |r| r.get(0),
            )
            .optional()?;
        if open.is_some() {
            return rule(format!("{} already has a reconciliation in progress", account.name));
        }
        let last: Option<NaiveDate> = tx.query_row(
            "SELECT MAX(statement_date) FROM reconciliations WHERE account_id = ?1 AND status = 'completed'",
            [account_id],
            |r| r.get(0),
        )?;
        if let Some(last) = last
            && statement_date <= last
        {
            return rule(format!("statement date must be after the last reconciled statement ({last})"));
        }
        tx.execute(
            "INSERT INTO reconciliations(account_id, statement_date, statement_balance) VALUES (?1, ?2, ?3)",
            params![account_id, statement_date, statement_balance.0],
        )?;
        let id = tx.last_insert_rowid();
        tx.execute(
            "UPDATE journal_lines SET reconciliation_id = ?1
             WHERE account_id = ?2 AND cleared = 'cleared' AND reconciliation_id IS NULL
               AND entry_id IN (SELECT id FROM journal_entries WHERE date <= ?3)",
            params![id, account_id, statement_date],
        )?;
        tx.commit()?;
        Ok(id)
    }

    pub fn reconciliation(&self, id: ReconciliationId) -> Result<Reconciliation> {
        reconciliation_in(&self.conn, id)
    }

    /// Unreconciled lines dated on or before the statement date.
    pub fn reconcile_candidates(&self, id: ReconciliationId) -> Result<Vec<ReconcileCandidate>> {
        let rec = self.reconciliation(id)?;
        let kind = self.account(rec.account_id)?.kind();
        let mut stmt = self.conn.prepare(
            "SELECT l.id, e.id, e.date, COALESCE(l.memo, e.memo), l.amount,
                    l.cleared = 'cleared' AND l.reconciliation_id IS ?3
             FROM journal_lines l JOIN journal_entries e ON e.id = l.entry_id
             WHERE l.account_id = ?1 AND l.cleared <> 'reconciled' AND e.date <= ?2
             ORDER BY e.date, e.id, l.id",
        )?;
        let rows = stmt
            .query_map(params![rec.account_id, rec.statement_date, id], |r| {
                Ok(ReconcileCandidate {
                    line_id: r.get(0)?,
                    entry_id: r.get(1)?,
                    date: r.get(2)?,
                    memo: r.get(3)?,
                    amount: kind.natural(Cents(r.get(4)?)),
                    checked: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn set_cleared(&mut self, id: ReconciliationId, line_id: LineId, checked: bool) -> Result<()> {
        let rec = self.reconciliation(id)?;
        if rec.status != ReconciliationStatus::InProgress {
            return rule("this reconciliation is already finished");
        }
        let line: Option<(AccountId, String, NaiveDate)> = self
            .conn
            .query_row(
                "SELECT l.account_id, l.cleared, e.date
                 FROM journal_lines l JOIN journal_entries e ON e.id = l.entry_id WHERE l.id = ?1",
                [line_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((account_id, cleared, date)) = line else {
            return Err(StoreError::NotFound("journal line", line_id));
        };
        if account_id != rec.account_id {
            return rule("that line belongs to a different account");
        }
        if cleared == Cleared::Reconciled.as_str() {
            return rule("that line was reconciled in an earlier statement");
        }
        if date > rec.statement_date {
            return rule("that line is dated after the statement date");
        }
        let (state, rec_id) = if checked { (Cleared::Cleared, Some(id)) } else { (Cleared::Uncleared, None) };
        self.conn.execute(
            "UPDATE journal_lines SET cleared = ?1, reconciliation_id = ?2 WHERE id = ?3",
            params![state.as_str(), rec_id, line_id],
        )?;
        Ok(())
    }

    pub fn reconciliation_summary(&self, id: ReconciliationId) -> Result<ReconcileSummary> {
        let rec = self.reconciliation(id)?;
        let kind = self.account(rec.account_id)?.kind();
        let previously: i64 = self.conn.query_row(
            "SELECT IFNULL(SUM(amount), 0) FROM journal_lines
             WHERE account_id = ?1 AND cleared = 'reconciled'",
            [rec.account_id],
            |r| r.get(0),
        )?;
        let cleared: i64 = self.conn.query_row(
            "SELECT IFNULL(SUM(amount), 0) FROM journal_lines
             WHERE reconciliation_id = ?1 AND cleared = 'cleared'",
            [id],
            |r| r.get(0),
        )?;
        Ok(ReconcileSummary::compute(kind, Cents(previously), [Cents(cleared)], rec.statement_balance))
    }

    /// Locks the checked lines as reconciled. Fails unless the difference is zero.
    pub fn finish_reconciliation(&mut self, id: ReconciliationId) -> Result<()> {
        let rec = self.reconciliation(id)?;
        if rec.status != ReconciliationStatus::InProgress {
            return rule("this reconciliation is already finished");
        }
        let summary = self.reconciliation_summary(id)?;
        if !summary.is_balanced() {
            return rule(format!("still off by {}; the difference must be 0.00 to finish", summary.difference));
        }
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE journal_lines SET cleared = 'reconciled' WHERE reconciliation_id = ?1 AND cleared = 'cleared'",
            [id],
        )?;
        tx.execute(
            "UPDATE reconciliations SET status = 'completed', completed_at = datetime('now') WHERE id = ?1",
            [id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Abandons an in-progress session. Checked lines stay cleared but detached.
    pub fn cancel_reconciliation(&mut self, id: ReconciliationId) -> Result<()> {
        let rec = self.reconciliation(id)?;
        if rec.status != ReconciliationStatus::InProgress {
            return rule("only an in-progress reconciliation can be cancelled");
        }
        let tx = self.conn.transaction()?;
        tx.execute("UPDATE journal_lines SET reconciliation_id = NULL WHERE reconciliation_id = ?1", [id])?;
        tx.execute("DELETE FROM reconciliations WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(())
    }

    /// Reopens the most recent completed statement for an account: its lines
    /// go back to cleared so they can be fixed and reconciled again.
    pub fn undo_last_reconciliation(&mut self, account_id: AccountId) -> Result<()> {
        let tx = self.conn.transaction()?;
        let open: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM reconciliations WHERE account_id = ?1 AND status = 'in_progress')",
            [account_id],
            |r| r.get(0),
        )?;
        if open {
            return rule("finish or cancel the reconciliation in progress first");
        }
        let last: Option<ReconciliationId> = tx
            .query_row(
                "SELECT id FROM reconciliations WHERE account_id = ?1 AND status = 'completed'
                 ORDER BY statement_date DESC, id DESC LIMIT 1",
                [account_id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(last) = last else {
            return rule("this account has no completed reconciliations");
        };
        tx.execute(
            "UPDATE journal_lines SET cleared = 'cleared', reconciliation_id = NULL WHERE reconciliation_id = ?1",
            [last],
        )?;
        tx.execute("DELETE FROM reconciliations WHERE id = ?1", [last])?;
        tx.commit()?;
        Ok(())
    }
}

// ---------------------------------------------------------------- shared helpers

fn post_in(tx: &Transaction, entry: &NewEntry, source: &str, reverses: Option<EntryId>) -> Result<EntryId> {
    entry.validate()?;
    check_not_locked(tx, entry.date)?;
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
fn bridge_lines(tx: &Transaction, business: Cents) -> Result<Vec<EntryLine>> {
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

fn reconciliation_in(conn: &Connection, id: ReconciliationId) -> Result<Reconciliation> {
    conn.query_row(
        "SELECT account_id, statement_date, statement_balance, status FROM reconciliations WHERE id = ?1",
        [id],
        |r| {
            let status: String = r.get(3)?;
            Ok(Reconciliation {
                id,
                account_id: r.get(0)?,
                statement_date: r.get(1)?,
                statement_balance: Cents(r.get(2)?),
                status: match status.as_str() {
                    "in_progress" => ReconciliationStatus::InProgress,
                    "completed" => ReconciliationStatus::Completed,
                    _ => return Err(conversion_err(3, BadValue(status))),
                },
            })
        },
    )
    .optional()?
    .ok_or(StoreError::NotFound("reconciliation", id))
}

impl Store {
    /// The open reconciliation session for an account, if any.
    pub fn active_reconciliation(&self, account_id: AccountId) -> Result<Option<Reconciliation>> {
        let id: Option<ReconciliationId> = self
            .conn
            .query_row(
                "SELECT id FROM reconciliations WHERE account_id = ?1 AND status = 'in_progress'",
                [account_id],
                |r| r.get(0),
            )
            .optional()?;
        id.map(|id| reconciliation_in(&self.conn, id)).transpose()
    }

    /// Completed statements for an account, newest first.
    pub fn reconciliations(&self, account_id: AccountId) -> Result<Vec<Reconciliation>> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM reconciliations WHERE account_id = ?1 AND status = 'completed'
             ORDER BY statement_date DESC, id DESC",
        )?;
        let ids: Vec<ReconciliationId> = stmt.query_map([account_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        ids.into_iter().map(|id| reconciliation_in(&self.conn, id)).collect()
    }

    /// Every `pref.*` setting, without the prefix.
    pub fn prefs(&self) -> Result<std::collections::BTreeMap<String, String>> {
        let mut stmt = self.conn.prepare("SELECT substr(key, 6), value FROM settings WHERE key LIKE 'pref.%'")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }
}
