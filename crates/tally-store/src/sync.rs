//! Device sync, the store's half (see `docs/SYNC.md`).
//!
//! A book can live on several devices. This module gives each device its own
//! range of row ids, records every change to a synced table in an outbox (by
//! triggers, in the change's own transaction), keeps a hybrid logical clock,
//! and applies another device's changes with the merge rules. Moving the
//! changes between devices, and encrypting them on the way, is the
//! `tally-sync` crate's job.

use crate::{Result, Store, StoreError, rule};
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OptionalExtension, ToSql, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

// ---------------------------------------------------------------- id ranges

/// Each device numbers its rows from `device × 2^40`, so two devices never
/// make the same id. Ids stay under 2^53, which JavaScript reads exactly.
pub const ID_RANGE_BITS: u32 = 40;
/// The highest device number a person's device can be given.
pub const MAX_DEVICE: i64 = 8190;
/// Device number 8191 belongs to no device: rows that every device derives
/// the same way while merging take their ids from its range.
pub const MERGE_DEVICE: i64 = 8191;

/// The ids device `device` may use: `device × 2^40` up to the next device's
/// range, except that device 0 starts at 1.
pub fn id_range(device: i64) -> (i64, i64) {
    let lo = if device == 0 { 1 } else { device << ID_RANGE_BITS };
    (lo, ((device + 1) << ID_RANGE_BITS) - 1)
}

/// A table that syncs, and how to name one of its rows.
pub struct SyncTable {
    pub name: &'static str,
    /// Primary key columns.
    pub pk: &'static [&'static str],
    /// An SQL condition on a row (`{r}` stands for `NEW` or `OLD`) that is
    /// true for rows that stay on this device.
    pub local_only: Option<&'static str>,
}

const fn table(name: &'static str, pk: &'static [&'static str]) -> SyncTable {
    SyncTable { name, pk, local_only: None }
}

/// Every table of the book that syncs. `docs/SYNC.md` says why each does.
/// A test fails if a table is in neither this list nor [`LOCAL_TABLES`].
pub const SYNCED_TABLES: &[SyncTable] = &[
    SyncTable {
        name: "settings",
        pk: &["key"],
        // This device's own preferences (cloud backup folder, ...) stay put.
        local_only: Some("{r}.\"key\" LIKE 'device.%' OR {r}.\"key\" LIKE 'pref.cloud.%'"),
    },
    table("accounts", &["id"]),
    table("journal_entries", &["id"]),
    table("journal_lines", &["id"]),
    table("reconciliations", &["id"]),
    table("bank_connections", &["id"]),
    table("bank_links", &["id"]),
    table("bank_transactions", &["id"]),
    table("rules", &["id"]),
    table("contacts", &["id"]),
    table("invoices", &["id"]),
    table("invoice_lines", &["id"]),
    table("bills", &["id"]),
    table("bill_lines", &["id"]),
    table("payments", &["id"]),
    table("payment_applications", &["id"]),
    table("account_tax_lines", &["account_id"]),
    table("tax_payments", &["id"]),
    table("people", &["id"]),
    table("budget_months", &["month"]),
    table("budgets", &["id"]),
    table("recurring_bills", &["id"]),
    table("recurring_bill_marks", &["bill_id", "due_date"]),
    table("goals", &["id"]),
    table("import_links", &["source", "kind", "external_id"]),
];

/// Tables that never leave this device: sync's own bookkeeping.
pub const LOCAL_TABLES: &[&str] = &["sync_state", "sync_outbox", "sync_rows", "sync_ids"];

fn synced_table(name: &str) -> Option<&'static SyncTable> {
    SYNCED_TABLES.iter().find(|t| t.name == name)
}

/// Tables whose rows are numbered by an `INTEGER PRIMARY KEY id`, which every
/// insert must take from [`insert_with_id`].
pub fn id_tables() -> impl Iterator<Item = &'static str> {
    SYNCED_TABLES.iter().filter(|t| t.pk == ["id"]).map(|t| t.name)
}

/// The next free id in this device's range for `table`. Ids only go up: a
/// deleted row's id is not handed out again, so a late edit to the old row
/// can't land on a new one.
fn next_id(conn: &Connection, table: &str) -> Result<i64> {
    debug_assert!(id_tables().any(|t| t == table), "{table} has no device id range");
    let device: i64 = conn.query_row("SELECT device FROM sync_state", [], |r| r.get(0))?;
    let (lo, hi) = id_range(device);
    let counter: Option<i64> = conn.query_row("SELECT next FROM sync_ids WHERE tbl = ?1", [table], |r| r.get(0)).optional()?;
    let live: Option<i64> =
        conn.query_row(&format!("SELECT MAX(id) FROM {table} WHERE id BETWEEN ?1 AND ?2"), [lo, hi], |r| r.get(0))?;
    let mut id = lo;
    if let Some(c) = counter.filter(|c| (lo..=hi).contains(c)) {
        id = id.max(c);
    }
    if let Some(m) = live {
        id = id.max(m + 1);
    }
    if id > hi {
        return rule("this device has used up its row ids");
    }
    conn.execute(
        "INSERT INTO sync_ids(tbl, next) VALUES (?1, ?2) ON CONFLICT(tbl) DO UPDATE SET next = excluded.next",
        params![table, id + 1],
    )?;
    Ok(id)
}

/// The one way to insert a row into a table numbered by `id`: runs `sql`
/// (`INSERT INTO <table>(..., id) VALUES (..., ?N)`, with `id` last and its
/// placeholder one past `params`) with an id from this device's range, and
/// returns that id. A test fails if an insert into such a table skips it.
pub(crate) fn insert_with_id(conn: &Connection, table: &str, sql: &str, params: &[&dyn ToSql]) -> Result<i64> {
    debug_assert!(sql.trim_start().starts_with(&format!("INSERT INTO {table}(")), "wrong table in {sql}");
    let id = next_id(conn, table)?;
    let mut all: Vec<&dyn ToSql> = params.to_vec();
    all.push(&id);
    conn.execute(sql, all.as_slice())?;
    Ok(id)
}

/// The id every device gives the `k`th row it derives from row `from` while
/// merging, drawn from the reserved merge range.
fn derived_id(from: i64, k: u32) -> i64 {
    let mut hash = Sha256::new();
    hash.update(b"tallybook merge id");
    hash.update(from.to_be_bytes());
    hash.update(k.to_be_bytes());
    let digest = hash.finalize();
    let mut low = [0u8; 8];
    low[3..].copy_from_slice(&digest[..5]);
    (MERGE_DEVICE << ID_RANGE_BITS) | (u64::from_be_bytes(low) as i64)
}

// ---------------------------------------------------------------- the clock

/// A hybrid logical clock reading: wall-clock milliseconds, a counter for
/// changes within the same millisecond (or after the clock went backwards),
/// and the device number to break ties. Readings order by those three, and
/// the text form orders the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Hlc {
    pub wall_ms: u64,
    pub counter: u32,
    pub device: u16,
}

impl Hlc {
    pub fn parse(s: &str) -> Option<Hlc> {
        let mut parts = s.split('-');
        let wall_ms = u64::from_str_radix(parts.next()?, 16).ok()?;
        let counter = u32::from_str_radix(parts.next()?, 16).ok()?;
        let device = u16::from_str_radix(parts.next()?, 16).ok()?;
        (parts.next().is_none() && s.len() == 30).then_some(Hlc { wall_ms, counter, device })
    }
}

impl std::fmt::Display for Hlc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:016x}-{:08x}-{:04x}", self.wall_ms, self.counter, self.device)
    }
}

// ---------------------------------------------------------------- triggers

/// Milliseconds since the epoch, in SQL.
const NOW_MS: &str = "(CAST(strftime('%s', 'now') AS INTEGER) * 1000 + CAST(strftime('%f', 'now') * 1000 AS INTEGER) % 1000)";
/// The clock reading after the one the trigger just advanced to, in SQL.
const HLC_NOW: &str = "(SELECT printf('%016x-%08x-%04x', hlc_wall, hlc_counter, device) FROM sync_state)";

fn advance_clock_sql() -> String {
    format!(
        "UPDATE sync_state SET hlc_counter = CASE WHEN {NOW_MS} > hlc_wall THEN 0 ELSE hlc_counter + 1 END, hlc_wall = max(hlc_wall, {NOW_MS});"
    )
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!("SELECT name FROM pragma_table_info('{table}') ORDER BY cid"))?;
    Ok(stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
}

fn json_object_sql(prefix: &str, columns: &[String]) -> String {
    let pairs: Vec<String> = columns.iter().map(|c| format!("'{c}', {prefix}.\"{c}\"")).collect();
    format!("json_object({})", pairs.join(", "))
}

fn json_pk_sql(prefix: &str, t: &SyncTable) -> String {
    let cols: Vec<String> = t.pk.iter().map(|c| format!("{prefix}.\"{c}\"")).collect();
    format!("json_array({})", cols.join(", "))
}

fn trigger_name(table: &str, kind: &str) -> String {
    format!("sync_{table}_{kind}")
}

/// Names of the triggers [`rebuild_triggers`] creates.
pub(crate) fn trigger_names() -> Vec<String> {
    SYNCED_TABLES.iter().flat_map(|t| ["ai", "au", "ad"].map(|k| trigger_name(t.name, k))).collect()
}

/// (Re)creates, for every synced table, the triggers that write each insert,
/// update and delete to the outbox. They are built from the table's columns as
/// they are now, so run this after any step that changes a table; the
/// migration runner does. A test checks every column is covered.
pub(crate) fn rebuild_triggers(conn: &Connection) -> Result<()> {
    for t in SYNCED_TABLES {
        let columns = table_columns(conn, t.name)?;
        if columns.is_empty() {
            return rule(format!("synced table {} is missing", t.name));
        }
        for kind in ["ai", "au", "ad"] {
            conn.execute_batch(&format!("DROP TRIGGER IF EXISTS {}", trigger_name(t.name, kind)))?;
        }
        let guard = |r: &str| {
            let mut when = "(SELECT applying = 0 AND enabled = 1 FROM sync_state)".to_string();
            if let Some(local) = t.local_only {
                when.push_str(&format!(" AND NOT ({})", local.replace("{r}", r)));
            }
            when
        };
        let bump = advance_clock_sql();
        let name = t.name;
        let new_pk = json_pk_sql("NEW", t);
        let old_pk = json_pk_sql("OLD", t);
        // An insert, or the row an update leaves behind.
        let put = |op: &str, born: &str, on_conflict: &str| {
            format!(
                "{bump}
                 INSERT INTO sync_outbox(tbl, pk, op, row, hlc) VALUES ('{name}', {new_pk}, '{op}', {row}, {HLC_NOW});
                 INSERT INTO sync_rows(tbl, pk, hlc, born, deleted) VALUES ('{name}', {new_pk}, {HLC_NOW}, {born}, 0)
                 ON CONFLICT(tbl, pk) DO UPDATE SET {on_conflict};",
                row = json_object_sql("NEW", &columns),
            )
        };
        conn.execute_batch(&format!(
            "CREATE TRIGGER {ai} AFTER INSERT ON {name} WHEN {g_new}
             BEGIN {insert} END;
             CREATE TRIGGER {au} AFTER UPDATE ON {name} WHEN {g_new}
             BEGIN {update}
                 INSERT INTO sync_outbox(tbl, pk, op, row, hlc)
                     SELECT '{name}', {old_pk}, 'delete', NULL, {HLC_NOW} WHERE {old_pk} IS NOT {new_pk};
                 INSERT OR REPLACE INTO sync_rows(tbl, pk, hlc, born, deleted)
                     SELECT '{name}', {old_pk}, {HLC_NOW}, {HLC_NOW}, 1 WHERE {old_pk} IS NOT {new_pk};
             END;
             CREATE TRIGGER {ad} AFTER DELETE ON {name} WHEN {g_old}
             BEGIN {bump}
                 INSERT INTO sync_outbox(tbl, pk, op, row, hlc) VALUES ('{name}', {old_pk}, 'delete', NULL, {HLC_NOW});
                 INSERT OR REPLACE INTO sync_rows(tbl, pk, hlc, born, deleted) VALUES ('{name}', {old_pk}, {HLC_NOW}, {HLC_NOW}, 1);
             END;",
            ai = trigger_name(name, "ai"),
            au = trigger_name(name, "au"),
            ad = trigger_name(name, "ad"),
            g_new = guard("NEW"),
            g_old = guard("OLD"),
            // A row first seen on insert is born then. A row first seen on
            // update was there before sync began, so it's the oldest kind.
            insert = put(
                "insert",
                HLC_NOW,
                "hlc = excluded.hlc, deleted = 0, born = CASE WHEN sync_rows.deleted = 1 THEN excluded.born ELSE sync_rows.born END"
            ),
            update = put("update", "''", "hlc = excluded.hlc, deleted = 0"),
        ))?;
    }
    Ok(())
}

// ---------------------------------------------------------------- changes

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeOp {
    Insert,
    Update,
    Delete,
}

impl ChangeOp {
    fn parse(s: &str) -> Option<ChangeOp> {
        match s {
            "insert" => Some(ChangeOp::Insert),
            "update" => Some(ChangeOp::Update),
            "delete" => Some(ChangeOp::Delete),
            _ => None,
        }
    }
}

/// One change to one row: what a device tells the others.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RowChange {
    pub table: String,
    /// The primary key's values, in the table's key order.
    pub pk: Vec<Value>,
    pub op: ChangeOp,
    /// The whole row after the change; `None` for a delete.
    pub row: Option<Map<String, Value>>,
    /// The device's clock reading when it made the change.
    pub hlc: String,
}

/// A change waiting in the outbox.
#[derive(Debug, Clone, PartialEq)]
pub struct OutboxRow {
    pub seq: i64,
    pub change: RowChange,
}

/// What happened to a batch of changes, for the person to be told about.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyReport {
    /// Changes that changed this device's rows.
    pub applied: usize,
    /// Changes that lost to a newer one, or that were already here.
    pub dropped: usize,
    /// Values renamed because two devices made the same unique value.
    pub renamed: Vec<Renamed>,
    /// Rows dropped because they clashed with an earlier row that must be unique.
    pub clashes_dropped: Vec<String>,
    /// Second voids of an entry, each cancelled by a reversal of its reversal.
    pub voids_merged: usize,
    /// Entries that arrived dated in a period this device has closed.
    pub in_closed_period: Vec<i64>,
    /// References to rows this device doesn't have (a parent deleted on one device and used on another).
    pub dangling_references: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Renamed {
    pub table: String,
    pub column: String,
    pub from: String,
    pub to: String,
}

fn bad(msg: impl Into<String>) -> StoreError {
    StoreError::Rule(format!("sync: {}", msg.into()))
}

fn json_to_sql(v: &Value) -> Result<SqlValue> {
    Ok(match v {
        Value::Null => SqlValue::Null,
        Value::Bool(b) => SqlValue::Integer(*b as i64),
        Value::Number(n) => match n.as_i64() {
            Some(i) => SqlValue::Integer(i),
            None => return Err(bad(format!("a number that isn't a whole number: {n}"))),
        },
        Value::String(s) => SqlValue::Text(s.clone()),
        other => return Err(bad(format!("unexpected value {other}"))),
    })
}

fn sql_to_json(v: SqlValue) -> Value {
    match v {
        SqlValue::Null => Value::Null,
        SqlValue::Integer(i) => Value::from(i),
        SqlValue::Real(f) => Value::from(f),
        SqlValue::Text(s) => Value::String(s),
        SqlValue::Blob(_) => Value::Null,
    }
}

/// The text SQLite's own `json_array` makes of a primary key, which is what
/// `sync_rows` is keyed on, so a key means the same thing everywhere.
fn pk_key(conn: &Connection, pk: &[Value]) -> Result<String> {
    let values = pk.iter().map(json_to_sql).collect::<Result<Vec<_>>>()?;
    let placeholders = (1..=values.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", ");
    Ok(conn.query_row(&format!("SELECT json_array({placeholders})"), rusqlite::params_from_iter(values), |r| r.get(0))?)
}

/// Where a row stands in `sync_rows`.
#[derive(Debug, Clone)]
struct RowState {
    hlc: String,
    born: String,

}

fn row_state(conn: &Connection, table: &str, key: &str) -> Result<Option<RowState>> {
    Ok(conn
        .query_row("SELECT hlc, born FROM sync_rows WHERE tbl = ?1 AND pk = ?2", params![table, key], |r| {
            Ok(RowState { hlc: r.get(0)?, born: r.get(1)? })
        })
        .optional()?)
}

fn write_state(conn: &Connection, table: &str, key: &str, hlc: &str, born: &str, deleted: bool) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO sync_rows(tbl, pk, hlc, born, deleted) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![table, key, hlc, born, deleted],
    )?;
    Ok(())
}

// ---------------------------------------------------------------- unique clashes

enum Clash {
    /// The later row gets `-2`, `-3`... after the value.
    Rename(&'static str),
    /// The later row is dropped.
    DropLater,
}

struct Unique {
    cols: &'static [&'static str],
    clash: Clash,
    /// Only rows with this column equal to this value are unique.
    only_when: Option<(&'static str, &'static str)>,
}

fn uniques(table: &str) -> &'static [Unique] {
    const ACCOUNTS: &[Unique] = &[Unique { cols: &["code"], clash: Clash::Rename("code"), only_when: None }];
    const INVOICES: &[Unique] = &[Unique { cols: &["number"], clash: Clash::Rename("number"), only_when: None }];
    const LINKS: &[Unique] = &[
        Unique { cols: &["ledger_account_id"], clash: Clash::DropLater, only_when: None },
        Unique { cols: &["connection_id", "external_account_id"], clash: Clash::DropLater, only_when: None },
    ];
    const TRANSACTIONS: &[Unique] =
        &[Unique { cols: &["ledger_account_id", "external_id"], clash: Clash::Rename("external_id"), only_when: None }];
    const RECONCILIATIONS: &[Unique] =
        &[Unique { cols: &["account_id"], clash: Clash::DropLater, only_when: Some(("status", "in_progress")) }];
    const BUDGETS: &[Unique] = &[
        Unique { cols: &["month", "person_id"], clash: Clash::DropLater, only_when: None },
        Unique { cols: &["month", "account_id"], clash: Clash::DropLater, only_when: None },
    ];
    match table {
        "accounts" => ACCOUNTS,
        "invoices" => INVOICES,
        "bank_links" => LINKS,
        "bank_transactions" => TRANSACTIONS,
        "reconciliations" => RECONCILIATIONS,
        "budgets" => BUDGETS,
        _ => &[],
    }
}

// ---------------------------------------------------------------- applying

/// Settings that belong to this device and never sync.
fn is_local_setting(key: &str) -> bool {
    key.starts_with("device.") || key.starts_with("pref.cloud.")
}

const JOURNAL_ENTRIES: &str = "journal_entries";
const JOURNAL_LINES: &str = "journal_lines";

/// The columns of the append-only tables that never change after insert (the
/// schema's triggers refuse edits to them).
fn immutable_columns(table: &str) -> &'static [&'static str] {
    match table {
        JOURNAL_ENTRIES => &["id", "date", "reverses_id"],
        JOURNAL_LINES => &["id", "entry_id", "account_id", "amount"],
        _ => &[],
    }
}

fn is_journal(table: &str) -> bool {
    table == JOURNAL_ENTRIES || table == JOURNAL_LINES
}

enum Outcome {
    Applied,
    Dropped,
}

struct Applier<'a> {
    conn: &'a Connection,
    report: ApplyReport,
    columns: BTreeMap<&'static str, Vec<String>>,
    /// Entries whose lines changed, to check they still sum to zero.
    touched_entries: BTreeSet<i64>,
    new_entries: Vec<(i64, String)>,
    highest_hlc: Option<Hlc>,
}

impl<'a> Applier<'a> {
    fn new(conn: &'a Connection) -> Applier<'a> {
        Applier {
            conn,
            report: ApplyReport::default(),
            columns: BTreeMap::new(),
            touched_entries: BTreeSet::new(),
            new_entries: Vec::new(),
            highest_hlc: None,
        }
    }

    fn columns_of(&mut self, t: &'static SyncTable) -> Result<Vec<String>> {
        if let Some(c) = self.columns.get(t.name) {
            return Ok(c.clone());
        }
        let c = table_columns(self.conn, t.name)?;
        self.columns.insert(t.name, c.clone());
        Ok(c)
    }

    fn apply(&mut self, c: &RowChange) -> Result<()> {
        let t = synced_table(&c.table).ok_or_else(|| bad(format!("a change to {}, which this version doesn't know", c.table)))?;
        if c.pk.len() != t.pk.len() {
            return Err(bad(format!("a key of the wrong size for {}", t.name)));
        }
        let hlc = Hlc::parse(&c.hlc).ok_or_else(|| bad(format!("a bad clock reading {:?}", c.hlc)))?;
        if self.highest_hlc.is_none_or(|h| hlc > h) {
            self.highest_hlc = Some(hlc);
        }
        let key = pk_key(self.conn, &c.pk)?;
        let state = row_state(self.conn, t.name, &key)?;
        let outcome = match c.op {
            ChangeOp::Delete => self.apply_delete(t, c, &key, state)?,
            ChangeOp::Insert | ChangeOp::Update => {
                let row = c.row.as_ref().ok_or_else(|| bad("an insert or update without a row"))?;
                self.check_columns(t, row)?;
                if is_journal(t.name) {
                    self.apply_journal(t, c, row, &key, state)?
                } else {
                    self.apply_put(t, c, row.clone(), &key, state)?
                }
            }
        };
        match outcome {
            Outcome::Applied => self.report.applied += 1,
            Outcome::Dropped => self.report.dropped += 1,
        }
        Ok(())
    }

    fn check_columns(&mut self, t: &'static SyncTable, row: &Map<String, Value>) -> Result<()> {
        let known = self.columns_of(t)?;
        if let Some(extra) = row.keys().find(|k| !known.contains(k)) {
            return Err(bad(format!("{}.{extra} isn't a column here; this book needs a newer Tallybook", t.name)));
        }
        Ok(())
    }

    fn exists(&self, t: &SyncTable, pk: &[Value]) -> Result<bool> {
        let cond: Vec<String> = t.pk.iter().enumerate().map(|(i, c)| format!("\"{c}\" = ?{}", i + 1)).collect();
        let values = pk.iter().map(json_to_sql).collect::<Result<Vec<_>>>()?;
        let sql = format!("SELECT EXISTS(SELECT 1 FROM {} WHERE {})", t.name, cond.join(" AND "));
        Ok(self.conn.query_row(&sql, rusqlite::params_from_iter(values), |r| r.get(0))?)
    }

    // ---- ordinary tables: the newest change to a row wins

    fn apply_delete(&mut self, t: &'static SyncTable, c: &RowChange, key: &str, state: Option<RowState>) -> Result<Outcome> {
        if is_journal(t.name) {
            // The ledger never deletes; a delete here is a bug or an attack.
            self.report.clashes_dropped.push(format!("ignored a delete of {} {key}", t.name));
            return Ok(Outcome::Dropped);
        }
        if state.as_ref().is_some_and(|s| s.hlc >= c.hlc) {
            return Ok(Outcome::Dropped);
        }
        let cond: Vec<String> = t.pk.iter().enumerate().map(|(i, col)| format!("\"{col}\" = ?{}", i + 1)).collect();
        let values = c.pk.iter().map(json_to_sql).collect::<Result<Vec<_>>>()?;
        self.conn.execute(&format!("DELETE FROM {} WHERE {}", t.name, cond.join(" AND ")), rusqlite::params_from_iter(values))?;
        let born = state.map(|s| s.born).unwrap_or_default();
        write_state(self.conn, t.name, key, &c.hlc, &born, true)?;
        Ok(Outcome::Applied)
    }

    fn apply_put(
        &mut self,
        t: &'static SyncTable,
        c: &RowChange,
        mut row: Map<String, Value>,
        key: &str,
        state: Option<RowState>,
    ) -> Result<Outcome> {
        if state.as_ref().is_some_and(|s| s.hlc >= c.hlc) {
            return Ok(Outcome::Dropped);
        }
        if t.name == "settings" && row.get("key").and_then(Value::as_str).is_some_and(is_local_setting) {
            return Ok(Outcome::Dropped);
        }
        let born = state.as_ref().map(|s| s.born.clone()).unwrap_or_else(|| c.hlc.clone());
        if let Outcome::Dropped = self.resolve_uniques(t, key, &born, &mut row)? {
            return Ok(Outcome::Dropped);
        }
        let names: Vec<String> = self.columns_of(t)?.into_iter().filter(|col| row.contains_key(col)).collect();
        let values = names.iter().map(|n| json_to_sql(&row[n])).collect::<Result<Vec<_>>>()?;
        let placeholders: Vec<String> = (1..=names.len()).map(|i| format!("?{i}")).collect();
        let quoted: Vec<String> = names.iter().map(|n| format!("\"{n}\"")).collect();
        let updates: Vec<String> =
            names.iter().filter(|n| !t.pk.contains(&n.as_str())).map(|n| format!("\"{n}\" = excluded.\"{n}\"")).collect();
        let pk_cols: Vec<String> = t.pk.iter().map(|p| format!("\"{p}\"")).collect();
        let conflict = if updates.is_empty() { "DO NOTHING".to_string() } else { format!("DO UPDATE SET {}", updates.join(", ")) };
        self.conn.execute(
            &format!(
                "INSERT INTO {} ({}) VALUES ({}) ON CONFLICT({}) {conflict}",
                t.name,
                quoted.join(", "),
                placeholders.join(", "),
                pk_cols.join(", ")
            ),
            rusqlite::params_from_iter(values),
        )?;
        write_state(self.conn, t.name, key, &c.hlc, &born, false)?;
        Ok(Outcome::Applied)
    }

    /// Settles a unique value that two devices made on their own. The later
    /// row (by when it was first seen, then by key, the same on every device)
    /// gives way: renamed, or dropped.
    fn resolve_uniques(&mut self, t: &'static SyncTable, key: &str, born: &str, row: &mut Map<String, Value>) -> Result<Outcome> {
        for u in uniques(t.name) {
            if let Some((col, value)) = u.only_when
                && row.get(col).and_then(Value::as_str) != Some(value)
            {
                continue;
            }
            if u.cols.iter().any(|c| row.get(*c).is_none_or(Value::is_null)) {
                continue;
            }
            let others = self.rows_with_same(t, u, row, key)?;
            if others.is_empty() {
                continue;
            }
            match u.clash {
                Clash::DropLater => {
                    for (okey, opk) in others {
                        let obirth = row_state(self.conn, t.name, &okey)?.map(|s| s.born).unwrap_or_default();
                        if (born, key) > (obirth.as_str(), okey.as_str()) {
                            self.report.clashes_dropped.push(format!("{} {key} clashed with {okey}", t.name));
                            return Ok(Outcome::Dropped);
                        }
                        self.drop_local_row(t, &opk, &okey, &obirth)?;
                        self.report.clashes_dropped.push(format!("{} {okey} clashed with {key}", t.name));
                    }
                }
                Clash::Rename(col) => self.rename_group(t, u, col, key, born, row)?,
            }
        }
        Ok(Outcome::Applied)
    }

    /// Existing rows (other than `key`) with the same values in `u`'s columns.
    fn rows_with_same(&self, t: &SyncTable, u: &Unique, row: &Map<String, Value>, key: &str) -> Result<Vec<(String, Vec<Value>)>> {
        let mut cond: Vec<String> = u.cols.iter().enumerate().map(|(i, c)| format!("\"{c}\" = ?{}", i + 1)).collect();
        let mut values = u.cols.iter().map(|c| json_to_sql(&row[*c])).collect::<Result<Vec<_>>>()?;
        if let Some((col, value)) = u.only_when {
            values.push(SqlValue::Text(value.to_string()));
            cond.push(format!("\"{col}\" = ?{}", values.len()));
        }
        let pk_cols: Vec<String> = t.pk.iter().map(|p| format!("\"{p}\"")).collect();
        let sql = format!("SELECT {} FROM {} WHERE {}", pk_cols.join(", "), t.name, cond.join(" AND "));
        let mut stmt = self.conn.prepare(&sql)?;
        let found: Vec<Vec<Value>> = stmt
            .query_map(rusqlite::params_from_iter(values), |r| {
                (0..t.pk.len()).map(|i| r.get::<_, SqlValue>(i).map(sql_to_json)).collect::<rusqlite::Result<Vec<_>>>()
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut out = Vec::new();
        for pk in found {
            let k = pk_key(self.conn, &pk)?;
            if k != key {
                out.push((k, pk));
            }
        }
        Ok(out)
    }

    /// Deletes a local row that lost a clash, and says so in `sync_rows`.
    fn drop_local_row(&mut self, t: &SyncTable, pk: &[Value], key: &str, born: &str) -> Result<()> {
        let cond: Vec<String> = t.pk.iter().enumerate().map(|(i, col)| format!("\"{col}\" = ?{}", i + 1)).collect();
        let values = pk.iter().map(json_to_sql).collect::<Result<Vec<_>>>()?;
        if t.name == "reconciliations" {
            self.conn.execute(
                "UPDATE journal_lines SET reconciliation_id = NULL,
                        cleared = CASE WHEN cleared = 'cleared' THEN 'uncleared' ELSE cleared END
                 WHERE reconciliation_id = ?1",
                params![json_to_sql(&pk[0])?],
            )?;
        }
        self.conn.execute(&format!("DELETE FROM {} WHERE {}", t.name, cond.join(" AND ")), rusqlite::params_from_iter(values))?;
        let hlc = row_state(self.conn, t.name, key)?.map(|s| s.hlc).unwrap_or_default();
        write_state(self.conn, t.name, key, &hlc, born, true)?;
        Ok(())
    }

    /// Gives every row that made the same value its place in line: the
    /// earliest keeps `X`, the next gets `X-2`, then `X-3`. The same rows give
    /// the same names on every device, whatever order they arrived in.
    fn rename_group(&mut self, t: &'static SyncTable, u: &Unique, col: &str, key: &str, born: &str, row: &mut Map<String, Value>) -> Result<()> {
        let base = row[col].as_str().ok_or_else(|| bad(format!("{} must be text", col)))?.to_string();
        let others_cols: Vec<&&str> = u.cols.iter().filter(|c| **c != col).collect();
        let mut cond: Vec<String> = others_cols.iter().enumerate().map(|(i, c)| format!("\"{c}\" = ?{}", i + 1)).collect();
        let mut values = others_cols.iter().map(|c| json_to_sql(&row[**c])).collect::<Result<Vec<_>>>()?;
        values.push(SqlValue::Text(base.clone()));
        values.push(SqlValue::Text(format!("{base}-*")));
        cond.push(format!("(\"{col}\" = ?{} OR \"{col}\" GLOB ?{})", values.len() - 1, values.len()));
        let pk_cols: Vec<String> = t.pk.iter().map(|p| format!("\"{p}\"")).collect();
        let sql = format!("SELECT {}, \"{col}\" FROM {} WHERE {}", pk_cols.join(", "), t.name, cond.join(" AND "));
        let mut stmt = self.conn.prepare(&sql)?;
        let found: Vec<(Vec<Value>, String)> = stmt
            .query_map(rusqlite::params_from_iter(values), |r| {
                let pk = (0..t.pk.len()).map(|i| r.get::<_, SqlValue>(i).map(sql_to_json)).collect::<rusqlite::Result<Vec<_>>>()?;
                Ok((pk, r.get::<_, String>(t.pk.len())?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        drop(stmt);
        // The group: rows named X or X-<digits>, and the incoming row.
        struct Member {
            born: String,
            key: String,
            pk: Option<Vec<Value>>,
            current: String,
        }
        let mut group = vec![Member { born: born.to_string(), key: key.to_string(), pk: None, current: base.clone() }];
        for (pk, current) in found {
            let suffix = current.strip_prefix(&base).and_then(|s| s.strip_prefix('-'));
            if current != base && !suffix.is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())) {
                continue;
            }
            let k = pk_key(self.conn, &pk)?;
            if k == key {
                continue;
            }
            let b = row_state(self.conn, t.name, &k)?.map(|s| s.born).unwrap_or_default();
            group.push(Member { born: b, key: k, pk: Some(pk), current });
        }
        group.sort_by(|a, b| (&a.born, &a.key).cmp(&(&b.born, &b.key)));
        // Rename from the last place to the first, so no two rows ever hold a name at once.
        for (rank, m) in group.iter().enumerate().rev() {
            let target = if rank == 0 { base.clone() } else { format!("{base}-{}", rank + 1) };
            if target == m.current {
                continue;
            }
            self.report.renamed.push(Renamed { table: t.name.to_string(), column: col.to_string(), from: m.current.clone(), to: target.clone() });
            match &m.pk {
                None => {
                    row.insert(col.to_string(), Value::String(target));
                    // A feed transaction that clashes is a duplicate of one already here:
                    // keep it out of the inbox, but keep the row so devices agree.
                    if t.name == "bank_transactions" && row.get("status").and_then(Value::as_str) == Some("new") {
                        row.insert("status".into(), Value::String("excluded".into()));
                    }
                }
                Some(pk) => {
                    let cond: Vec<String> = t.pk.iter().enumerate().map(|(i, p)| format!("\"{p}\" = ?{}", i + 2)).collect();
                    let mut values = vec![SqlValue::Text(target)];
                    for v in pk {
                        values.push(json_to_sql(v)?);
                    }
                    let extra = if t.name == "bank_transactions" { ", status = CASE WHEN status = 'new' THEN 'excluded' ELSE status END" } else { "" };
                    self.conn.execute(
                        &format!("UPDATE {} SET \"{col}\" = ?1{extra} WHERE {}", t.name, cond.join(" AND ")),
                        rusqlite::params_from_iter(values),
                    )?;
                }
            }
        }
        Ok(())
    }

    // ---- the journal: insert if absent, edit only what may be edited

    fn apply_journal(&mut self, t: &'static SyncTable, c: &RowChange, row: &Map<String, Value>, key: &str, state: Option<RowState>) -> Result<Outcome> {
        let id = c.pk[0].as_i64().ok_or_else(|| bad("a journal id that isn't a number"))?;
        if !self.exists(t, &c.pk)? {
            let names: Vec<String> = self.columns_of(t)?.into_iter().filter(|col| row.contains_key(col)).collect();
            let values = names.iter().map(|n| json_to_sql(&row[n])).collect::<Result<Vec<_>>>()?;
            let placeholders: Vec<String> = (1..=names.len()).map(|i| format!("?{i}")).collect();
            let quoted: Vec<String> = names.iter().map(|n| format!("\"{n}\"")).collect();
            self.conn.execute(
                &format!("INSERT INTO {} ({}) VALUES ({})", t.name, quoted.join(", "), placeholders.join(", ")),
                rusqlite::params_from_iter(values),
            )?;
            write_state(self.conn, t.name, key, &c.hlc, &c.hlc, false)?;
            if t.name == JOURNAL_ENTRIES {
                self.touched_entries.insert(id);
                let date = row.get("date").and_then(Value::as_str).unwrap_or_default().to_string();
                self.new_entries.push((id, date));
            } else if let Some(entry_id) = row.get("entry_id").and_then(Value::as_i64) {
                self.touched_entries.insert(entry_id);
            }
            return Ok(Outcome::Applied);
        }
        // Already here. Only the columns the ledger lets change can differ.
        let mut changed = false;
        if t.name == JOURNAL_ENTRIES && let Some(incoming) = row.get("voided_by_id").and_then(Value::as_i64) {
            changed |= self.merge_void(id, incoming)?;
        }
        let newer = state.as_ref().is_none_or(|s| s.hlc < c.hlc);
        if newer {
            let skip = immutable_columns(t.name);
            let names: Vec<String> = self
                .columns_of(t)?
                .into_iter()
                .filter(|col| row.contains_key(col) && !skip.contains(&col.as_str()) && !(t.name == JOURNAL_ENTRIES && col == "voided_by_id"))
                .collect();
            if !names.is_empty() {
                let sets: Vec<String> = names.iter().enumerate().map(|(i, n)| format!("\"{n}\" = ?{}", i + 1)).collect();
                let mut values = names.iter().map(|n| json_to_sql(&row[n])).collect::<Result<Vec<_>>>()?;
                values.push(json_to_sql(&c.pk[0])?);
                let n = values.len();
                changed |= self.conn.execute(
                    &format!("UPDATE {} SET {} WHERE id = ?{n}", t.name, sets.join(", ")),
                    rusqlite::params_from_iter(values),
                )? > 0;
            }
            let born = state.map(|s| s.born).unwrap_or_default();
            write_state(self.conn, t.name, key, &c.hlc, &born, false)?;
        }
        Ok(if changed { Outcome::Applied } else { Outcome::Dropped })
    }

    /// Two devices voided the same entry. The void made first (the earlier
    /// reversing entry) stands; the other reversal is itself reversed, by a
    /// row every device derives the same way, so the books net to one void.
    /// Returns whether this entry's void changed.
    fn merge_void(&mut self, entry_id: i64, incoming: i64) -> Result<bool> {
        let current: Option<i64> = self.conn.query_row("SELECT voided_by_id FROM journal_entries WHERE id = ?1", [entry_id], |r| r.get(0))?;
        let Some(current) = current else {
            self.conn.execute("UPDATE journal_entries SET voided_by_id = ?1 WHERE id = ?2", params![incoming, entry_id])?;
            return Ok(true);
        };
        if current == incoming {
            return Ok(false);
        }
        let rank = |id: i64| -> Result<(String, i64)> {
            let key = pk_key(self.conn, &[Value::from(id)])?;
            Ok((row_state(self.conn, JOURNAL_ENTRIES, &key)?.map(|s| s.born).unwrap_or_default(), id))
        };
        let (winner, loser) = if rank(current)? <= rank(incoming)? { (current, incoming) } else { (incoming, current) };
        let changed = winner != current;
        if changed {
            self.conn.execute("UPDATE journal_entries SET voided_by_id = ?1 WHERE id = ?2", params![winner, entry_id])?;
        }
        self.report.voids_merged += 1;
        self.cancel_reversal(loser)?;
        Ok(changed)
    }

    /// Reverses the reversing entry `reversal` (unless something already has),
    /// with ids derived from it so every device makes the identical rows.
    fn cancel_reversal(&mut self, reversal: i64) -> Result<()> {
        let head: Option<(String, String, Option<i64>, String)> = self
            .conn
            .query_row("SELECT date, memo, voided_by_id, created_at FROM journal_entries WHERE id = ?1", [reversal], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .optional()?;
        let Some((date, memo, voided_by, created_at)) = head else {
            return Err(bad(format!("a void by entry {reversal}, which hasn't arrived")));
        };
        if voided_by.is_some() {
            return Ok(());
        }
        let new_id = derived_id(reversal, 0);
        let born = row_state(self.conn, JOURNAL_ENTRIES, &pk_key(self.conn, &[Value::from(reversal)])?)?.map(|s| s.born).unwrap_or_default();
        let hlc = row_state(self.conn, JOURNAL_ENTRIES, &pk_key(self.conn, &[Value::from(reversal)])?)?.map(|s| s.hlc).unwrap_or_default();
        let taken: bool = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM journal_entries WHERE id = ?1)", [new_id], |r| r.get(0))?;
        if taken {
            return Err(bad(format!("entry id {new_id} is already used by something else")));
        }
        self.conn.execute(
            "INSERT INTO journal_entries(id, date, memo, source, reverses_id, created_at) VALUES (?1, ?2, ?3, 'void', ?4, ?5)",
            params![new_id, date, format!("Duplicate void cancelled: {memo}"), reversal, created_at],
        )?;
        write_state(self.conn, JOURNAL_ENTRIES, &pk_key(self.conn, &[Value::from(new_id)])?, &hlc, &born, false)?;
        let lines: Vec<(i64, i64, i64, Option<String>)> = {
            let mut stmt = self.conn.prepare("SELECT id, account_id, amount, memo FROM journal_lines WHERE entry_id = ?1 ORDER BY id")?;
            stmt.query_map([reversal], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<_>>()?
        };
        for (i, (_, account_id, amount, line_memo)) in lines.into_iter().enumerate() {
            let line_id = derived_id(reversal, i as u32 + 1);
            self.conn.execute(
                "INSERT INTO journal_lines(id, entry_id, account_id, amount, memo) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![line_id, new_id, account_id, -amount, line_memo],
            )?;
            write_state(self.conn, JOURNAL_LINES, &pk_key(self.conn, &[Value::from(line_id)])?, &hlc, &born, false)?;
        }
        self.conn.execute("UPDATE journal_entries SET voided_by_id = ?1 WHERE id = ?2", params![new_id, reversal])?;
        self.touched_entries.insert(new_id);
        Ok(())
    }
}

// ---------------------------------------------------------------- the Store API

/// Which rows of a synced table, as JSON objects sorted by key: how tests
/// (and a curious person) compare two copies of a book.
pub type TableDump = BTreeMap<String, Vec<String>>;

impl Store {
    /// Turns sync on for this book: from now on every change to a synced
    /// table is recorded in the outbox. Changes made before are not (a
    /// snapshot carries them to a new device).
    pub fn sync_enable(&self) -> Result<()> {
        self.conn.execute("UPDATE sync_state SET enabled = 1", [])?;
        Ok(())
    }

    pub fn sync_enabled(&self) -> Result<bool> {
        Ok(self.conn.query_row("SELECT enabled FROM sync_state", [], |r| r.get(0))?)
    }

    /// This device's number in the book: 0 for the original.
    pub fn sync_device(&self) -> Result<u16> {
        Ok(self.conn.query_row("SELECT device FROM sync_state", [], |r| r.get(0))?)
    }

    /// The last batch sequence applied.
    pub fn sync_last_seq(&self) -> Result<i64> {
        Ok(self.conn.query_row("SELECT last_seq FROM sync_state", [], |r| r.get(0))?)
    }

    /// The clock's current reading.
    pub fn sync_clock(&self) -> Result<Hlc> {
        let (wall, counter, device): (i64, i64, i64) =
            self.conn.query_row("SELECT hlc_wall, hlc_counter, device FROM sync_state", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(Hlc { wall_ms: wall as u64, counter: counter as u32, device: device as u16 })
    }

    /// Makes this copy of a book a new device: numbered `device` (assigned by
    /// Connect when it joined), with sync on, an empty outbox, and
    /// `last_seq` as the point its snapshot was taken at. For a book just
    /// restored from a snapshot.
    pub fn sync_become_device(&mut self, device: u16, last_seq: i64) -> Result<()> {
        if i64::from(device) > MAX_DEVICE {
            return rule("that device number is out of range");
        }
        let tx = self.conn.savepoint()?;
        tx.execute("DELETE FROM sync_outbox", [])?;
        tx.execute("UPDATE sync_state SET device = ?1, enabled = 1, applying = 0, last_seq = ?2", params![device, last_seq])?;
        tx.commit()?;
        Ok(())
    }

    /// The oldest changes waiting to be pushed, up to about `max` of them. A
    /// batch never ends inside an entry: the lines that follow an entry are
    /// included, so a device always receives whole entries.
    pub fn sync_outbox_batch(&self, max: usize) -> Result<Vec<OutboxRow>> {
        let mut stmt = self.conn.prepare("SELECT seq, tbl, pk, op, row, hlc FROM sync_outbox WHERE seq > ?1 ORDER BY seq LIMIT ?2")?;
        let read = |r: &rusqlite::Row| -> rusqlite::Result<(i64, String, String, String, Option<String>, String)> {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
        };
        let mut raw: Vec<_> = stmt.query_map(params![0, max as i64], read)?.collect::<rusqlite::Result<_>>()?;
        loop {
            let after = raw.last().map_or(0, |r| r.0);
            let next = stmt.query_map(params![after, 1], read)?.next().transpose()?;
            match next {
                Some(r) if r.1 == JOURNAL_LINES && r.3 == "insert" && !raw.is_empty() => raw.push(r),
                _ => break,
            }
        }
        raw.into_iter()
            .map(|(seq, table, pk, op, row, hlc)| {
                let pk: Vec<Value> = serde_json::from_str(&pk).map_err(|e| bad(format!("a bad key in the outbox: {e}")))?;
                let row = row
                    .map(|r| serde_json::from_str::<Map<String, Value>>(&r))
                    .transpose()
                    .map_err(|e| bad(format!("a bad row in the outbox: {e}")))?;
                let op = ChangeOp::parse(&op).ok_or_else(|| bad("a bad operation in the outbox"))?;
                Ok(OutboxRow { seq, change: RowChange { table, pk, op, row, hlc } })
            })
            .collect()
    }

    /// How many changes are waiting to be pushed.
    pub fn sync_outbox_len(&self) -> Result<i64> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get(0))?)
    }

    /// Forgets outbox rows up to and including `seq`, once they're pushed.
    pub fn sync_outbox_done(&self, seq: i64) -> Result<()> {
        self.conn.execute("DELETE FROM sync_outbox WHERE seq <= ?1", [seq])?;
        Ok(())
    }

    /// Applies other devices' changes in one transaction, following the merge
    /// rules in `docs/SYNC.md`, then checks the books still balance. If they
    /// don't, nothing is applied and the error says so. `through_seq`, when
    /// given, is recorded as the last batch sequence applied, in the same
    /// transaction, so a crash can't apply a batch twice or skip it.
    ///
    /// The outbox triggers are off while this runs (nothing echoes back), and
    /// so is foreign-key enforcement, since a row can arrive before the row it
    /// points at is settled. The ledger's own triggers stay on: nothing here
    /// edits an entry's date or a line's amount, and a void is replaced only
    /// by another void.
    pub fn sync_apply(&mut self, changes: &[RowChange], through_seq: Option<i64>) -> Result<ApplyReport> {
        let foreign_keys: bool = self.conn.pragma_query_value(None, "foreign_keys", |r| r.get(0))?;
        self.conn.pragma_update(None, "foreign_keys", false)?;
        let result = self.sync_apply_inner(changes, through_seq);
        self.conn.pragma_update(None, "foreign_keys", foreign_keys)?;
        result
    }

    fn sync_apply_inner(&mut self, changes: &[RowChange], through_seq: Option<i64>) -> Result<ApplyReport> {
        let tx = self.conn.savepoint()?;
        let applying: bool = tx.query_row("SELECT applying FROM sync_state", [], |r| r.get(0))?;
        if applying {
            return rule("sync is already applying changes");
        }
        tx.execute("UPDATE sync_state SET applying = 1", [])?;
        let mut applier = Applier::new(&tx);
        for change in changes {
            applier.apply(change)?;
        }
        let Applier { mut report, touched_entries, new_entries, highest_hlc, .. } = applier;

        if let Some(lock) = crate::lock_date_in(&tx)? {
            for (id, date) in new_entries {
                if date.parse::<chrono::NaiveDate>().is_ok_and(|d| d <= lock) {
                    report.in_closed_period.push(id);
                }
            }
        }
        // The books must still balance: every entry that changed sums to zero,
        // and so does the whole journal.
        for id in touched_entries {
            let sum: i64 = tx.query_row("SELECT IFNULL(SUM(amount), 0) FROM journal_lines WHERE entry_id = ?1", [id], |r| r.get(0))?;
            if sum != 0 {
                return rule(format!("sync would leave entry #{id} out of balance by {sum}; nothing was applied"));
            }
        }
        let total: i64 = tx.query_row("SELECT IFNULL(SUM(amount), 0) FROM journal_lines", [], |r| r.get(0))?;
        if total != 0 {
            return rule(format!("sync would leave the books out of balance by {total}; nothing was applied"));
        }
        report.dangling_references = {
            let mut stmt = tx.prepare("PRAGMA foreign_key_check")?;
            stmt.query_map([], |_| Ok(()))?.count()
        };

        if let Some(seen) = highest_hlc {
            let now = {
                let (w, c): (i64, i64) = tx.query_row("SELECT hlc_wall, hlc_counter FROM sync_state", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
                (w as u64, c as u32)
            };
            if (seen.wall_ms, seen.counter) > now {
                tx.execute("UPDATE sync_state SET hlc_wall = ?1, hlc_counter = ?2", params![seen.wall_ms as i64, seen.counter])?;
            }
        }
        if let Some(seq) = through_seq {
            tx.execute("UPDATE sync_state SET last_seq = max(last_seq, ?1)", [seq])?;
        }
        tx.execute("UPDATE sync_state SET applying = 0", [])?;
        tx.commit()?;
        Ok(report)
    }

    /// Every row of every synced table, as JSON, sorted by key. Two devices
    /// that have traded all their changes have identical dumps.
    pub fn sync_dump(&self) -> Result<TableDump> {
        let mut out = TableDump::new();
        for t in SYNCED_TABLES {
            let columns = table_columns(&self.conn, t.name)?;
            let order: Vec<String> = t.pk.iter().map(|c| format!("\"{c}\"")).collect();
            let filter = t.local_only.map(|l| format!(" WHERE NOT ({})", l.replace("{r}", t.name))).unwrap_or_default();
            let sql = format!("SELECT {} FROM {}{filter} ORDER BY {}", json_object_sql(t.name, &columns), t.name, order.join(", "));
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<_>>()?;
            out.insert(t.name.to_string(), rows);
        }
        Ok(out)
    }

    /// Columns of synced tables that the outbox triggers don't write (as
    /// `table.column`). Empty when every column is covered; a column added
    /// without rebuilding the triggers shows up here.
    pub fn sync_uncovered_columns(&self) -> Result<Vec<String>> {
        let mut missing = Vec::new();
        for t in SYNCED_TABLES {
            for kind in ["ai", "au"] {
                let sql: Option<String> = self
                    .conn
                    .query_row("SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = ?1", [trigger_name(t.name, kind)], |r| r.get(0))
                    .optional()?;
                let Some(sql) = sql else {
                    missing.push(format!("{}: no {kind} trigger", t.name));
                    continue;
                };
                for c in table_columns(&self.conn, t.name)? {
                    if !sql.contains(&format!("'{c}', NEW.\"{c}\"")) {
                        missing.push(format!("{}.{c}", t.name));
                    }
                }
            }
        }
        missing.sort();
        missing.dedup();
        Ok(missing)
    }

    /// Tables in the book that are neither synced nor sync's own bookkeeping.
    /// A table added without deciding which it is shows up here.
    pub fn sync_unclassified_tables(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY name")?;
        let names: Vec<String> = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(names.into_iter().filter(|n| synced_table(n).is_none() && !LOCAL_TABLES.contains(&n.as_str())).collect())
    }

    /// Writes a consistent plain copy of the book to `path`, for a snapshot
    /// that a new device starts from. The sync key encrypts it before it
    /// leaves this device (see `tally-sync`), so it is plain here even when
    /// the book is encrypted on disk.
    pub fn sync_snapshot_to(&self, path: &std::path::Path) -> Result<()> {
        if self.key.is_some() {
            self.export_plain(path)
        } else {
            if path.exists() {
                return rule(format!("{} already exists", path.display()));
            }
            self.conn.execute("VACUUM INTO ?1", [path.to_string_lossy()])?;
            Ok(())
        }
    }
}
