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
use crate::sync_apply::Applier;
use std::collections::BTreeMap;

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
pub const LOCAL_TABLES: &[&str] = &["sync_state", "sync_outbox", "sync_rows", "sync_ids", "sync_renames", "sync_shadow"];

pub(crate) fn synced_table(name: &str) -> Option<&'static SyncTable> {
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
pub(crate) fn derived_id(from: i64, k: u32) -> i64 {
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

pub(crate) fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!("SELECT name FROM pragma_table_info('{table}') ORDER BY cid"))?;
    Ok(stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
}

/// `json_object` of a row. A column that was renamed to settle a clash is
/// written with the value the row was made with, so every device ranks the
/// rows by what each one wanted, not by what the last clash made of it.
fn json_object_sql(prefix: &str, t: &SyncTable, columns: &[String], outbox: bool) -> String {
    let renamed = contention(t.name).and_then(|c| match c.policy {
        Contention::Rename(col) => Some(col),
        Contention::SetAside => None,
    });
    let pk = json_pk_sql(prefix, t);
    let pairs: Vec<String> = columns
        .iter()
        .map(|c| {
            if outbox && renamed == Some(c.as_str()) {
                format!(
                    "'{c}', COALESCE((SELECT intended FROM sync_renames WHERE tbl = '{t}' AND pk = {pk} AND col = '{c}' AND shown = {prefix}.\"{c}\"), {prefix}.\"{c}\")",
                    t = t.name
                )
            } else {
                format!("'{c}', {prefix}.\"{c}\"")
            }
        })
        .collect();
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
                row = json_object_sql("NEW", t, &columns, true),
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
    /// Rows set aside because an earlier row has the same unique value (a
    /// second budget for a month, a duplicate bank transaction...).
    pub set_aside: Vec<String>,
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

pub(crate) fn bad(msg: impl Into<String>) -> StoreError {
    StoreError::Rule(format!("sync: {}", msg.into()))
}

pub(crate) fn json_to_sql(v: &Value) -> Result<SqlValue> {
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

pub(crate) fn sql_to_json(v: SqlValue) -> Value {
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
pub(crate) fn pk_key(conn: &Connection, pk: &[Value]) -> Result<String> {
    let values = pk.iter().map(json_to_sql).collect::<Result<Vec<_>>>()?;
    let placeholders = (1..=values.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", ");
    Ok(conn.query_row(&format!("SELECT json_array({placeholders})"), rusqlite::params_from_iter(values), |r| r.get(0))?)
}

/// Where a row stands in `sync_rows`.
#[derive(Debug, Clone)]
pub(crate) struct RowState {
    pub(crate) hlc: String,
    pub(crate) born: String,
    pub(crate) deleted: bool,
}

pub(crate) fn row_state(conn: &Connection, table: &str, key: &str) -> Result<Option<RowState>> {
    Ok(conn
        .query_row("SELECT hlc, born, deleted FROM sync_rows WHERE tbl = ?1 AND pk = ?2", params![table, key], |r| {
            Ok(RowState { hlc: r.get(0)?, born: r.get(1)?, deleted: r.get(2)? })
        })
        .optional()?)
}

pub(crate) fn write_state(conn: &Connection, table: &str, key: &str, hlc: &str, born: &str, deleted: bool) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO sync_rows(tbl, pk, hlc, born, deleted) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![table, key, hlc, born, deleted],
    )?;
    Ok(())
}

// ---------------------------------------------------------------- unique values

/// What happens when two devices make rows that want the same unique value.
#[derive(Clone, Copy)]
pub(crate) enum Contention {
    /// The later row gets `-2`, `-3`... after the value; the person is told.
    Rename(&'static str),
    /// The earliest row keeps it and the rest wait aside (in `sync_shadow`),
    /// coming back if what kept them out goes away.
    SetAside,
}

pub(crate) struct Unique {
    pub(crate) cols: &'static [&'static str],
    /// Only rows with this column equal to this value are unique.
    pub(crate) only_when: Option<(&'static str, &'static str)>,
}

impl Unique {
    /// Whether `row` takes part: the unique columns are all set, and any condition holds.
    pub(crate) fn applies_to(&self, row: &Map<String, Value>) -> bool {
        if let Some((col, value)) = self.only_when
            && row.get(col).and_then(Value::as_str) != Some(value)
        {
            return false;
        }
        self.cols.iter().all(|c| row.get(*c).is_some_and(|v| !v.is_null()))
    }
}

pub(crate) struct Rules {
    pub(crate) policy: Contention,
    pub(crate) uniques: &'static [Unique],
}

impl Rules {
    /// Whether two rows want the same unique value.
    pub(crate) fn wants_same(&self, a: &Map<String, Value>, b: &Map<String, Value>) -> bool {
        self.uniques.iter().any(|u| u.applies_to(a) && u.applies_to(b) && u.cols.iter().all(|c| a[*c] == b[*c]))
    }

    pub(crate) fn what(&self) -> String {
        self.uniques.iter().map(|u| u.cols.join("+")).collect::<Vec<_>>().join(" or ")
    }
}

/// The synced tables with a unique value other than their key, and how a
/// clash is settled. A name a person reads (an account code, an invoice
/// number) is renamed; a row that is only ever one-per-thing (a budget for a
/// month, an open reconciliation, a bank transaction, a bank link) keeps
/// the earliest and sets the rest aside.
pub(crate) fn contention(table: &str) -> Option<&'static Rules> {
    static ACCOUNTS: Rules = Rules { policy: Contention::Rename("code"), uniques: &[Unique { cols: &["code"], only_when: None }] };
    static INVOICES: Rules = Rules { policy: Contention::Rename("number"), uniques: &[Unique { cols: &["number"], only_when: None }] };
    static LINKS: Rules = Rules {
        policy: Contention::SetAside,
        uniques: &[Unique { cols: &["ledger_account_id"], only_when: None }, Unique { cols: &["connection_id", "external_account_id"], only_when: None }],
    };
    static TRANSACTIONS: Rules =
        Rules { policy: Contention::SetAside, uniques: &[Unique { cols: &["ledger_account_id", "external_id"], only_when: None }] };
    static RECONCILIATIONS: Rules =
        Rules { policy: Contention::SetAside, uniques: &[Unique { cols: &["account_id"], only_when: Some(("status", "in_progress")) }] };
    static BUDGETS: Rules = Rules {
        policy: Contention::SetAside,
        uniques: &[Unique { cols: &["month", "person_id"], only_when: None }, Unique { cols: &["month", "account_id"], only_when: None }],
    };
    match table {
        "accounts" => Some(&ACCOUNTS),
        "invoices" => Some(&INVOICES),
        "bank_links" => Some(&LINKS),
        "bank_transactions" => Some(&TRANSACTIONS),
        "reconciliations" => Some(&RECONCILIATIONS),
        "budgets" => Some(&BUDGETS),
        _ => None,
    }
}

/// Settings that belong to this device and never sync.
pub(crate) fn is_local_setting(key: &str) -> bool {
    key.starts_with("device.") || key.starts_with("pref.cloud.")
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
                Some(r) if r.1 == "journal_lines" && r.3 == "insert" && !raw.is_empty() => raw.push(r),
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
        applier.settle()?;
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

    /// Works out again which rows keep the unique values they want and which
    /// are renamed or set aside, after changes made on this device (a delete
    /// can end a clash). Nothing goes to the outbox.
    pub fn sync_settle(&mut self) -> Result<ApplyReport> {
        self.sync_apply(&[], None)
    }

    /// How many rows are waiting aside for a unique value an earlier row has.
    pub fn sync_set_aside_count(&self) -> Result<i64> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM sync_shadow", [], |r| r.get(0))?)
    }

    /// Every row of every synced table, as JSON, sorted by key. Two devices
    /// that have traded all their changes have identical dumps.
    pub fn sync_dump(&self) -> Result<TableDump> {
        let mut out = TableDump::new();
        for t in SYNCED_TABLES {
            let columns = table_columns(&self.conn, t.name)?;
            let order: Vec<String> = t.pk.iter().map(|c| format!("\"{c}\"")).collect();
            let filter = t.local_only.map(|l| format!(" WHERE NOT ({})", l.replace("{r}", t.name))).unwrap_or_default();
            let sql = format!("SELECT {} FROM {}{filter} ORDER BY {}", json_object_sql(t.name, t, &columns, false), t.name, order.join(", "));
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
                    if !(sql.contains(&format!("'{c}', ")) && sql.contains(&format!("NEW.\"{c}\""))) {
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
