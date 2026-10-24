//! Applying another device's changes (the merge rules of `docs/SYNC.md`).
//!
//! The rules are written so that the result depends only on the changes, not
//! on the order they arrive in or how they are grouped into batches: the
//! newest change to a row wins (by clock reading, then device), a delete is
//! a change like another, the journal is insert-if-absent, and values that
//! must be unique are settled by a ranking of the rows that want them
//! (which row was made first), recomputed from all the rows each time, so a
//! clash that goes away (one row deleted) leaves no trace.

use crate::sync::*;
use crate::Result;
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

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

/// A row in the running for a unique value.
struct Contender {
    pk: Vec<Value>,
    key: String,
    born: String,
    row: Map<String, Value>,
    in_main: bool,
}

pub(crate) struct Applier<'a> {
    conn: &'a Connection,
    pub(crate) report: ApplyReport,
    columns: BTreeMap<&'static str, Vec<String>>,
    /// Entries whose lines changed, to check they still sum to zero.
    pub(crate) touched_entries: BTreeSet<i64>,
    pub(crate) new_entries: Vec<(i64, String)>,
    pub(crate) highest_hlc: Option<Hlc>,
    /// Unique values to re-settle: (table, intended value).
    bases: BTreeSet<(&'static str, String)>,
    /// Rows of tables with set-aside rows that changed.
    touched_rows: BTreeMap<&'static str, Vec<Vec<Value>>>,
}

impl<'a> Applier<'a> {
    pub(crate) fn new(conn: &'a Connection) -> Applier<'a> {
        Applier {
            conn,
            report: ApplyReport::default(),
            columns: BTreeMap::new(),
            touched_entries: BTreeSet::new(),
            new_entries: Vec::new(),
            highest_hlc: None,
            bases: BTreeSet::new(),
            touched_rows: BTreeMap::new(),
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

    pub(crate) fn apply(&mut self, c: &RowChange) -> Result<()> {
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

    fn pk_condition(t: &SyncTable, first_param: usize) -> String {
        t.pk.iter().enumerate().map(|(i, c)| format!("\"{c}\" = ?{}", first_param + i)).collect::<Vec<_>>().join(" AND ")
    }

    fn pk_params(pk: &[Value]) -> Result<Vec<SqlValue>> {
        pk.iter().map(json_to_sql).collect()
    }

    fn exists(&self, t: &SyncTable, pk: &[Value]) -> Result<bool> {
        let sql = format!("SELECT EXISTS(SELECT 1 FROM {} WHERE {})", t.name, Self::pk_condition(t, 1));
        Ok(self.conn.query_row(&sql, rusqlite::params_from_iter(Self::pk_params(pk)?), |r| r.get(0))?)
    }

    fn get_row(&mut self, t: &'static SyncTable, pk: &[Value]) -> Result<Option<Map<String, Value>>> {
        let cols = self.columns_of(t)?;
        let pairs: Vec<String> = cols.iter().map(|c| format!("'{c}', \"{c}\"")).collect();
        let sql = format!("SELECT json_object({}) FROM {} WHERE {}", pairs.join(", "), t.name, Self::pk_condition(t, 1));
        let text: Option<String> =
            self.conn.query_row(&sql, rusqlite::params_from_iter(Self::pk_params(pk)?), |r| r.get(0)).optional()?;
        text.map(|s| serde_json::from_str(&s).map_err(|e| bad(e.to_string()))).transpose()
    }

    fn delete_row(&self, t: &SyncTable, pk: &[Value]) -> Result<()> {
        let sql = format!("DELETE FROM {} WHERE {}", t.name, Self::pk_condition(t, 1));
        self.conn.execute(&sql, rusqlite::params_from_iter(Self::pk_params(pk)?))?;
        Ok(())
    }

    /// Inserts the row, or replaces the columns it carries if its key exists.
    fn upsert_row(&mut self, t: &'static SyncTable, row: &Map<String, Value>) -> Result<()> {
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
        Ok(())
    }

    // ---- ordinary tables: the newest change to a row wins

    fn apply_delete(&mut self, t: &'static SyncTable, c: &RowChange, key: &str, state: Option<RowState>) -> Result<Outcome> {
        if is_journal(t.name) {
            // The ledger never deletes; a delete here is a bug or an attack.
            self.report.set_aside.push(format!("ignored a delete of {} {key}", t.name));
            return Ok(Outcome::Dropped);
        }
        if state.as_ref().is_some_and(|s| s.hlc >= c.hlc) {
            return Ok(Outcome::Dropped);
        }
        self.forget(t, &c.pk, key)?;
        let born = state.map(|s| s.born).unwrap_or_default();
        write_state(self.conn, t.name, key, &c.hlc, &born, true)?;
        Ok(Outcome::Applied)
    }

    /// Removes a row from wherever it is (the table, or set aside), noting
    /// the unique values that now have to be settled again.
    fn forget(&mut self, t: &'static SyncTable, pk: &[Value], key: &str) -> Result<()> {
        if let Some(Contention::Rename(col)) = contention(t.name).map(|c| c.policy) {
            self.note_leaving(t, col, pk, key)?;
        }
        self.conn.execute("DELETE FROM sync_renames WHERE tbl = ?1 AND pk = ?2", params![t.name, key])?;
        if contention(t.name).is_some() {
            self.conn.execute("DELETE FROM sync_shadow WHERE tbl = ?1 AND pk = ?2", params![t.name, key])?;
            self.touched_rows.entry(t.name).or_default();
        }
        self.delete_row(t, pk)
    }

    fn apply_put(&mut self, t: &'static SyncTable, c: &RowChange, mut row: Map<String, Value>, key: &str, state: Option<RowState>) -> Result<Outcome> {
        if state.as_ref().is_some_and(|s| s.hlc >= c.hlc) {
            return Ok(Outcome::Dropped);
        }
        if t.name == "settings" && row.get("key").and_then(Value::as_str).is_some_and(is_local_setting) {
            return Ok(Outcome::Dropped);
        }
        // A row first seen as an update was there before sync began: the oldest kind.
        let born = match &state {
            Some(s) if !(s.deleted && c.op == ChangeOp::Insert) => s.born.clone(),
            Some(_) => c.hlc.clone(),
            None if c.op == ChangeOp::Insert => c.hlc.clone(),
            None => String::new(),
        };
        match contention(t.name).map(|c| c.policy) {
            None => self.upsert_row(t, &row)?,
            Some(Contention::Rename(col)) => self.put_renaming(t, col, &c.pk, key, &mut row)?,
            Some(Contention::SetAside) => self.put_setting_aside(t, &c.pk, key, &row)?,
        }
        write_state(self.conn, t.name, key, &c.hlc, &born, false)?;
        Ok(Outcome::Applied)
    }

    // ---- values that must be unique, and are renamed (-2, -3...) when clashing

    /// What the row at `pk` is meant to be called: the value it was made with,
    /// which differs from what it's called if it was renamed.
    fn intended(&self, t: &SyncTable, col: &str, key: &str, current: &str) -> Result<String> {
        let renamed: Option<String> = self
            .conn
            .query_row(
                "SELECT intended FROM sync_renames WHERE tbl = ?1 AND pk = ?2 AND col = ?3 AND shown = ?4",
                params![t.name, key, col, current],
                |r| r.get(0),
            )
            .optional()?;
        Ok(renamed.unwrap_or_else(|| current.to_string()))
    }

    fn current_value(&self, t: &SyncTable, col: &str, pk: &[Value]) -> Result<Option<String>> {
        let sql = format!("SELECT \"{col}\" FROM {} WHERE {}", t.name, Self::pk_condition(t, 1));
        Ok(self.conn.query_row(&sql, rusqlite::params_from_iter(Self::pk_params(pk)?), |r| r.get::<_, Option<String>>(0)).optional()?.flatten())
    }

    /// A row is leaving, or changing its value: its intended value's group changes.
    fn note_leaving(&mut self, t: &'static SyncTable, col: &str, pk: &[Value], key: &str) -> Result<()> {
        if let Some(current) = self.current_value(t, col, pk)? {
            let intended = self.intended(t, col, key, &current)?;
            self.bases.insert((t.name, intended));
        }
        Ok(())
    }

    fn put_renaming(&mut self, t: &'static SyncTable, col: &'static str, pk: &[Value], key: &str, row: &mut Map<String, Value>) -> Result<()> {
        self.note_leaving(t, col, pk, key)?;
        // The image carries the value the row was made with; any rename here is superseded.
        self.conn.execute("DELETE FROM sync_renames WHERE tbl = ?1 AND pk = ?2", params![t.name, key])?;
        let Some(value) = row.get(col).and_then(Value::as_str).map(str::to_string) else {
            return self.upsert_row(t, row);
        };
        // Make room: whoever holds the value now steps aside for the moment.
        let holder: Option<Vec<Value>> = {
            let pk_cols: Vec<String> = t.pk.iter().map(|p| format!("\"{p}\"")).collect();
            let sql = format!("SELECT {} FROM {} WHERE \"{col}\" = ?1", pk_cols.join(", "), t.name);
            let mut stmt = self.conn.prepare(&sql)?;
            let found: Vec<Vec<Value>> = stmt
                .query_map([&value], |r| (0..t.pk.len()).map(|i| r.get::<_, SqlValue>(i).map(sql_to_json)).collect::<rusqlite::Result<Vec<_>>>())?
                .collect::<rusqlite::Result<_>>()?;
            found.into_iter().find(|p| p != pk)
        };
        if let Some(other) = holder {
            let other_key = pk_key(self.conn, &other)?;
            let intended = self.intended(t, col, &other_key, &value)?;
            let temp = format!("\u{1}{other_key}");
            let sql = format!("UPDATE {} SET \"{col}\" = ?1 WHERE {}", t.name, Self::pk_condition(t, 2));
            let mut params = vec![SqlValue::Text(temp.clone())];
            params.extend(Self::pk_params(&other)?);
            self.conn.execute(&sql, rusqlite::params_from_iter(params))?;
            self.record_rename(t, &other_key, col, &intended, &temp)?;
            self.bases.insert((t.name, intended));
        }
        self.upsert_row(t, row)?;
        self.bases.insert((t.name, value));
        Ok(())
    }

    fn record_rename(&self, t: &SyncTable, key: &str, col: &str, intended: &str, shown: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sync_renames(tbl, pk, col, intended, shown) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(tbl, pk, col) DO UPDATE SET intended = excluded.intended, shown = excluded.shown",
            params![t.name, key, col, intended, shown],
        )?;
        Ok(())
    }

    /// Gives every row that wants `base` its place in line: the earliest made
    /// keeps `base`, the next gets `base-2`, then `base-3`. The same rows give
    /// the same names on every device, whatever order they arrived in, and a
    /// row that no longer has to step aside gets its own name back.
    fn settle_base(&mut self, t: &'static SyncTable, col: &'static str, base: &str) -> Result<()> {
        // Members: rows called `base` that weren't renamed, and rows renamed away from it.
        let mut members: Vec<(Vec<Value>, String, String)> = Vec::new(); // pk, key, current
        {
            let pk_cols: Vec<String> = t.pk.iter().map(|p| format!("\"{p}\"")).collect();
            let sql = format!("SELECT {} FROM {} WHERE \"{col}\" = ?1", pk_cols.join(", "), t.name);
            let mut stmt = self.conn.prepare(&sql)?;
            let found: Vec<Vec<Value>> = stmt
                .query_map([base], |r| (0..t.pk.len()).map(|i| r.get::<_, SqlValue>(i).map(sql_to_json)).collect::<rusqlite::Result<Vec<_>>>())?
                .collect::<rusqlite::Result<_>>()?;
            for pk in found {
                let key = pk_key(self.conn, &pk)?;
                if self.intended(t, col, &key, base)? == base {
                    members.push((pk, key, base.to_string()));
                }
            }
            let mut stmt = self.conn.prepare("SELECT pk, shown FROM sync_renames WHERE tbl = ?1 AND col = ?2 AND intended = ?3")?;
            let renamed: Vec<(String, String)> =
                stmt.query_map(params![t.name, col, base], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
            for (key, shown) in renamed {
                let pk: Vec<Value> = serde_json::from_str(&key).map_err(|e| bad(e.to_string()))?;
                if self.current_value(t, col, &pk)?.as_deref() == Some(shown.as_str()) {
                    members.push((pk, key, shown));
                } else {
                    self.conn.execute("DELETE FROM sync_renames WHERE tbl = ?1 AND pk = ?2 AND col = ?3", params![t.name, key, col])?;
                }
            }
        }
        if members.is_empty() {
            return Ok(());
        }
        let mut ranked: Vec<(String, String, Vec<Value>, String)> = Vec::new(); // born, key, pk, current
        for (pk, key, current) in members {
            let born = row_state(self.conn, t.name, &key)?.map(|s| s.born).unwrap_or_default();
            ranked.push((born, key, pk, current));
        }
        ranked.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        let member_keys: BTreeSet<String> = ranked.iter().map(|r| r.1.clone()).collect();
        // Names go in order: the base, then base-2, base-3 (skipping any a row
        // outside the group already has by that name).
        let mut targets = vec![base.to_string()];
        let mut k = 2;
        while targets.len() < ranked.len() {
            let candidate = format!("{base}-{k}");
            k += 1;
            if !self.name_held_outside(t, col, &candidate, &member_keys)? {
                targets.push(candidate);
            }
        }
        let changing: Vec<usize> = (0..ranked.len()).filter(|&i| ranked[i].3 != targets[i]).collect();
        let update = format!("UPDATE {} SET \"{col}\" = ?1 WHERE {}", t.name, Self::pk_condition(t, 2));
        for &i in &changing {
            let mut params = vec![SqlValue::Text(format!("\u{1}{}", ranked[i].1))];
            params.extend(Self::pk_params(&ranked[i].2)?);
            self.conn.execute(&update, rusqlite::params_from_iter(params))?;
        }
        for &i in &changing {
            let mut params = vec![SqlValue::Text(targets[i].clone())];
            params.extend(Self::pk_params(&ranked[i].2)?);
            self.conn.execute(&update, rusqlite::params_from_iter(params))?;
            if !ranked[i].3.starts_with('\u{1}') && targets[i] != base {
                self.report.renamed.push(Renamed { table: t.name.to_string(), column: col.to_string(), from: base.to_string(), to: targets[i].clone() });
            }
        }
        for (i, r) in ranked.iter().enumerate() {
            if targets[i] == base {
                self.conn.execute("DELETE FROM sync_renames WHERE tbl = ?1 AND pk = ?2 AND col = ?3", params![t.name, r.1, col])?;
            } else {
                self.record_rename(t, &r.1, col, base, &targets[i])?;
            }
        }
        Ok(())
    }

    /// Whether a row outside the group has `name` as its own (not a rename).
    fn name_held_outside(&self, t: &SyncTable, col: &str, name: &str, group: &BTreeSet<String>) -> Result<bool> {
        let pk_cols: Vec<String> = t.pk.iter().map(|p| format!("\"{p}\"")).collect();
        let sql = format!("SELECT {} FROM {} WHERE \"{col}\" = ?1", pk_cols.join(", "), t.name);
        let mut stmt = self.conn.prepare(&sql)?;
        let found: Vec<Vec<Value>> = stmt
            .query_map([name], |r| (0..t.pk.len()).map(|i| r.get::<_, SqlValue>(i).map(sql_to_json)).collect::<rusqlite::Result<Vec<_>>>())?
            .collect::<rusqlite::Result<_>>()?;
        for pk in found {
            let key = pk_key(self.conn, &pk)?;
            if !group.contains(&key) && self.intended(t, col, &key, name)? == name {
                return Ok(true);
            }
        }
        Ok(false)
    }

    // ---- values that must be unique, where one row keeps them and the rest are set aside

    fn put_setting_aside(&mut self, t: &'static SyncTable, pk: &[Value], key: &str, row: &Map<String, Value>) -> Result<()> {
        self.touched_rows.entry(t.name).or_default().push(pk.to_vec());
        let aside: bool = self
            .conn
            .query_row("SELECT EXISTS(SELECT 1 FROM sync_shadow WHERE tbl = ?1 AND pk = ?2)", params![t.name, key], |r| r.get(0))?;
        if aside {
            return self.shadow_put(t, key, row);
        }
        if self.clashing(t, row, key)?.is_empty() {
            return self.upsert_row(t, row);
        }
        // Wanted by a row already here: wait aside until the ranking is settled.
        self.delete_row(t, pk)?;
        self.shadow_put(t, key, row)
    }

    fn shadow_put(&self, t: &SyncTable, key: &str, row: &Map<String, Value>) -> Result<()> {
        let text = serde_json::to_string(row).map_err(|e| bad(e.to_string()))?;
        self.conn.execute("INSERT OR REPLACE INTO sync_shadow(tbl, pk, row) VALUES (?1, ?2, ?3)", params![t.name, key, text])?;
        Ok(())
    }

    /// Rows in the table, other than `key`, that want the same unique values as `row`.
    fn clashing(&mut self, t: &'static SyncTable, row: &Map<String, Value>, key: &str) -> Result<Vec<Vec<Value>>> {
        let Some(c) = contention(t.name) else { return Ok(vec![]) };
        let mut out: Vec<Vec<Value>> = Vec::new();
        for u in c.uniques {
            if !u.applies_to(row) {
                continue;
            }
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
            for pk in found {
                if pk_key(self.conn, &pk)? != key && !out.contains(&pk) {
                    out.push(pk);
                }
            }
        }
        Ok(out)
    }

    /// Decides which of the rows that want the same unique values keeps them:
    /// the earliest made, then the rest in turn. A row set aside comes back
    /// when what kept it out goes away.
    fn settle_set_aside(&mut self, t: &'static SyncTable) -> Result<()> {
        let Some(rules) = contention(t.name) else { return Ok(()) };
        let mut pool: Vec<Contender> = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        // Seeds: every row set aside, and every row that changed.
        let aside: Vec<(String, String)> = {
            let mut stmt = self.conn.prepare("SELECT pk, row FROM sync_shadow WHERE tbl = ?1")?;
            stmt.query_map([t.name], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
        };
        for (key, text) in aside {
            let pk: Vec<Value> = serde_json::from_str(&key).map_err(|e| bad(e.to_string()))?;
            let row: Map<String, Value> = serde_json::from_str(&text).map_err(|e| bad(e.to_string()))?;
            let born = row_state(self.conn, t.name, &key)?.map(|s| s.born).unwrap_or_default();
            seen.insert(key.clone());
            pool.push(Contender { pk, key, born, row, in_main: false });
        }
        for pk in self.touched_rows.remove(t.name).unwrap_or_default() {
            let key = pk_key(self.conn, &pk)?;
            if seen.contains(&key) {
                continue;
            }
            if let Some(row) = self.get_row(t, &pk)? {
                let born = row_state(self.conn, t.name, &key)?.map(|s| s.born).unwrap_or_default();
                seen.insert(key.clone());
                pool.push(Contender { pk, key, born, row, in_main: true });
            }
        }
        if pool.is_empty() {
            return Ok(());
        }
        // Everyone in the table who wants a value a contender wants.
        let mut next = 0;
        while next < pool.len() {
            let (row, key) = (pool[next].row.clone(), pool[next].key.clone());
            next += 1;
            for pk in self.clashing(t, &row, &key)? {
                let k = pk_key(self.conn, &pk)?;
                if seen.insert(k.clone()) {
                    let Some(row) = self.get_row(t, &pk)? else { continue };
                    let born = row_state(self.conn, t.name, &k)?.map(|s| s.born).unwrap_or_default();
                    pool.push(Contender { pk, key: k, born, row, in_main: true });
                }
            }
        }
        pool.sort_by(|a, b| (&a.born, &a.key).cmp(&(&b.born, &b.key)));
        let mut admitted: Vec<usize> = Vec::new();
        for i in 0..pool.len() {
            if !admitted.iter().any(|&j| rules.wants_same(&pool[i].row, &pool[j].row)) {
                admitted.push(i);
            }
        }
        // Rows leaving the table first, so the ones coming in find room.
        for (i, c) in pool.iter().enumerate() {
            if c.in_main && !admitted.contains(&i) {
                self.delete_row(t, &c.pk)?;
                self.shadow_put(t, &c.key, &c.row)?;
                self.report.set_aside.push(format!("{} {} was set aside: another row has the same {}", t.name, c.key, rules.what()));
            }
        }
        for &i in &admitted {
            let c = &pool[i];
            if !c.in_main {
                self.conn.execute("DELETE FROM sync_shadow WHERE tbl = ?1 AND pk = ?2", params![t.name, c.key])?;
                let row = c.row.clone();
                self.upsert_row(t, &row)?;
            }
        }
        Ok(())
    }

    /// Settles every unique value that changed or that is waiting on another.
    pub(crate) fn settle(&mut self) -> Result<()> {
        for t in SYNCED_TABLES {
            match contention(t.name).map(|c| c.policy) {
                Some(Contention::Rename(col)) => {
                    // Drop renames whose row is gone, then settle every group that changed or is still renamed.
                    let renamed: Vec<(String, String)> = {
                        let mut stmt = self.conn.prepare("SELECT pk, intended FROM sync_renames WHERE tbl = ?1 AND col = ?2")?;
                        stmt.query_map(params![t.name, col], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
                    };
                    for (_, intended) in renamed {
                        self.bases.insert((t.name, intended));
                    }
                    let bases: Vec<String> = self.bases.iter().filter(|(table, _)| *table == t.name).map(|(_, b)| b.clone()).collect();
                    for base in bases {
                        self.settle_base(t, col, &base)?;
                    }
                }
                Some(Contention::SetAside) => self.settle_set_aside(t)?,
                None => {}
            }
        }
        Ok(())
    }

    // ---- the journal: insert if absent, edit only what may be edited

    fn apply_journal(&mut self, t: &'static SyncTable, c: &RowChange, row: &Map<String, Value>, key: &str, state: Option<RowState>) -> Result<Outcome> {
        let id = c.pk[0].as_i64().ok_or_else(|| bad("a journal id that isn't a number"))?;
        if !self.exists(t, &c.pk)? {
            self.upsert_row(t, row)?;
            let born = if c.op == ChangeOp::Insert { c.hlc.clone() } else { String::new() };
            write_state(self.conn, t.name, key, &c.hlc, &born, false)?;
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
        if t.name == JOURNAL_ENTRIES
            && let Some(incoming) = row.get("voided_by_id").and_then(Value::as_i64)
        {
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
        let state = row_state(self.conn, JOURNAL_ENTRIES, &pk_key(self.conn, &[Value::from(reversal)])?)?;
        let (born, hlc) = state.map(|s| (s.born, s.hlc)).unwrap_or_default();
        let taken: bool = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM journal_entries WHERE id = ?1)", [new_id], |r| r.get(0))?;
        if taken {
            return Err(bad(format!("entry id {new_id} is already used by something else")));
        }
        self.conn.execute(
            "INSERT INTO journal_entries(id, date, memo, source, reverses_id, created_at) VALUES (?1, ?2, ?3, 'void', ?4, ?5)",
            params![new_id, date, format!("Duplicate void cancelled: {memo}"), reversal, created_at],
        )?;
        write_state(self.conn, JOURNAL_ENTRIES, &pk_key(self.conn, &[Value::from(new_id)])?, &hlc, &born, false)?;
        let lines: Vec<(i64, i64, Option<String>)> = {
            let mut stmt = self.conn.prepare("SELECT account_id, amount, memo FROM journal_lines WHERE entry_id = ?1 ORDER BY id")?;
            stmt.query_map([reversal], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?
        };
        for (i, (account_id, amount, line_memo)) in lines.into_iter().enumerate() {
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
