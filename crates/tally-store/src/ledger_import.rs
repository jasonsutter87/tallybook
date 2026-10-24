//! Bringing a household over from Ledger.
//!
//! Ledger keeps a single-entry list: signed amounts, each tagged with who it
//! was for and what it was (a "sector"), with no notion of which bank paid.
//! Here every entry becomes a balanced entry against one stand-in money
//! account, people stay people, sectors become household categories, stores
//! become rules, monthly allocations become person budgets, recurring items
//! become bills, and goals keep their contributions.
//!
//! The source is opened read-only and never changed. Importing again adds
//! only what's new: every row brought in is remembered in `import_links`
//! (see `SOURCE` for what's kept there).
//!
//! What's read from Ledger: tables `person`, `entry`, `month` and
//! `allocation`, plus `sector`, `goal`, `store` and `recurring` when present.
//! Columns some Ledger versions lack are read as empty. Ids are text;
//! amounts are signed integer cents (spending negative); dates are `YYYY-MM-DD`.

use crate::{insert_with_id, NewAccount, NewGoal, NewPerson, NewRecurringBill, NewRule, PersonId, Result, Store, StoreError, rule};
use chrono::{Datelike, Days, NaiveDate};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use tally_core::chart::codes;
use tally_core::rules::{Direction as RuleDirection, MatchOp};
use tally_core::schedule::Cadence;
use tally_core::{AccountId, AccountKind, AccountSubtype, Cents, EntryLine, Flex, NewEntry, Scope};

/// `import_links` rows from here say `source = 'ledger'`. Each row is
/// (`kind`, Ledger's id as `external_id`, what it became as `local_id`):
///
/// * `person`: a person id.
/// * `sector`: an account id (a category, or the savings account).
/// * `goal`: a goal id.
/// * `entry`: a journal entry id.
/// * `month`: a `YYYY-MM`; `local_id` is always `MONTH_LINK`.
/// * `bill`: a recurring bill id (from Ledger's `recurring` table).
/// * `store`: the id of the rule made from it.
/// * `money`, `savings`: external id `STAND_IN`; the account Ledger's money,
///   or its savings, moved through.
const SOURCE: &str = "ledger";
/// External id for the accounts that stand in for things Ledger never had.
const STAND_IN: &str = "default";
/// `local_id` for a month: it has no row id here.
const MONTH_LINK: i64 = 0;
/// `journal_entries.source` for entries brought in from Ledger.
const ENTRY_SOURCE: &str = "ledger_import";
/// Every rule made from a Ledger store gets this priority (lower is tried
/// first), leaving room to put the user's own rules ahead of them.
const RULE_PRIORITY: i64 = 100;
/// Store names shorter than this would match too much bank text as a rule.
const MIN_RULE_PATTERN_LEN: usize = 3;

/// What's in a Ledger database, before bringing it in.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LedgerPreview {
    pub people: usize,
    pub entries: usize,
    pub months: usize,
    pub goals: usize,
    pub bills: usize,
    pub stores: usize,
    pub first: Option<NaiveDate>,
    pub last: Option<NaiveDate>,
}

/// What an import brought in, counted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LedgerImport {
    pub people: usize,
    pub categories_created: usize,
    pub entries: usize,
    /// Already brought in by an earlier import.
    pub entries_skipped: usize,
    pub goals: usize,
    pub bills: usize,
    pub rules: usize,
    pub months: usize,
    /// The stand-in account every imported entry moves money through.
    pub money_account_id: AccountId,
    /// Things that didn't carry over exactly, in plain words.
    pub notes: Vec<String>,
}

/// One row of Ledger's `entry` table.
struct Entry {
    id: String,
    person: Option<String>,
    store: Option<String>,
    goal: Option<String>,
    sector: Option<String>,
    date: NaiveDate,
    amount: i64,
    note: String,
}

/// A Ledger database, open read-only.
struct Source {
    conn: Connection,
}

impl Source {
    fn open(path: &Path) -> Result<Source> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
            .map_err(|e| StoreError::Rule(format!("couldn't open {}: {e}", path.display())))?;
        // Someone else's file: run nothing its schema asks for, and never write.
        crate::harden(&conn)?;
        conn.pragma_update(None, "query_only", true)?;
        let src = Source { conn };
        for t in ["person", "entry", "month"] {
            if !src.has_table(t)? {
                return rule(format!("{} doesn't look like a Ledger database (no {t} table)", path.display()));
            }
        }
        Ok(src)
    }

    fn has_table(&self, name: &str) -> Result<bool> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1", [name], |r| r.get::<_, i64>(0))? > 0)
    }

    fn has_column(&self, table: &str, column: &str) -> Result<bool> {
        let mut stmt = self.conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let cols = stmt.query_map([], |r| r.get::<_, String>(1))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(cols.iter().any(|c| c == column))
    }

    fn count(&self, table: &str) -> Result<usize> {
        if !self.has_table(table)? {
            return Ok(0);
        }
        Ok(self.conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get::<_, i64>(0))? as usize)
    }

    fn entries(&self) -> Result<Vec<Entry>> {
        let opt = |col: &str| -> Result<String> { Ok(if self.has_column("entry", col)? { col.to_string() } else { "NULL".into() }) };
        let sql = format!(
            "SELECT id, person_id, {}, {}, {}, occurred_on, amount_cents, note FROM entry ORDER BY occurred_on, created_at, id",
            opt("store_id")?,
            opt("goal_id")?,
            opt("sector_id")?
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Entry {
                    id: r.get(0)?,
                    person: r.get(1)?,
                    store: r.get(2)?,
                    goal: r.get(3)?,
                    sector: r.get(4)?,
                    date: r.get(5)?,
                    amount: r.get(6)?,
                    note: r.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }
}

/// Ledger's default sectors, and names people commonly give them, mapped to
/// household chart codes.
const SECTOR_CODES: &[(&str, &str)] = &[
    ("groceries", "H6200"),
    ("dining", "H6210"),
    ("dining out", "H6210"),
    ("eating out", "H6210"),
    ("coffee", "H6220"),
    ("household", "H6030"),
    ("home", "H6030"),
    ("transport", "H6320"),
    ("gas", "H6320"),
    ("fuel", "H6320"),
    ("health", "H6410"),
    ("medical", "H6410"),
    ("personal care", "H6710"),
    ("kids", "H6510"),
    ("fun", "H6740"),
    ("entertainment", "H6740"),
    ("clothing", "H6700"),
    ("clothes", "H6700"),
    ("pets", "H6600"),
    ("gifts", "H6750"),
    ("travel", "H6760"),
    ("rent", "H6000"),
    ("utilities", "H6100"),
    ("phone", "H6130"),
    ("subscriptions", "H6730"),
    ("shopping", "H6720"),
];

/// Ledger's "changeability" for a sector: how far its spending can bend.
fn flex_of(changeability: &str) -> Flex {
    match changeability {
        "low" => Flex::Fixed,
        "high" => Flex::Lifestyle,
        _ => Flex::Usage,
    }
}

/// A Ledger store (a payee), kept for memos and to become a rule.
struct LedgerStore {
    name: String,
    sector: Option<String>,
    person: Option<String>,
    archived: bool,
}

/// Ledger's stores by id. Optional in older Ledger databases.
fn read_stores(src: &Source) -> Result<HashMap<String, LedgerStore>> {
    let mut stores = HashMap::new();
    if src.has_table("store")? {
        let mut stmt = src.conn.prepare("SELECT id, name, sector_id, default_person_id, archived_at FROM store")?;
        for row in stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, Option<String>>(3)?, r.get::<_, Option<String>>(4)?))
        })? {
            let (id, name, sector, person, archived) = row?;
            stores.insert(id, LedgerStore { name, sector, person, archived: archived.is_some() });
        }
    }
    Ok(stores)
}

impl Store {
    /// What a Ledger row of `kind` with id `external` became here, if it's
    /// been brought in before.
    fn link(&self, kind: &str, external: &str) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT local_id FROM import_links WHERE source = ?1 AND kind = ?2 AND external_id = ?3",
                params![SOURCE, kind, external],
                |r| r.get(0),
            )
            .optional()?)
    }

    fn remember(&self, kind: &str, external: &str, local: i64) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO import_links(source, kind, external_id, local_id) VALUES (?1, ?2, ?3, ?4)",
            params![SOURCE, kind, external, local],
        )?;
        Ok(())
    }

    fn code_id(&self, code: &str) -> Result<AccountId> {
        self.account_by_code(code)?.map(|a| a.id).ok_or_else(|| StoreError::Rule(format!("account {code} is missing from the household chart")))
    }

    /// Finds a household account of a kind by name, or makes one, and
    /// remembers it under `kind`/`STAND_IN` so a later import uses the same one.
    fn household_account(&self, kind: &str, name: &str, subtype: AccountSubtype, find: impl Fn(&tally_core::Account) -> bool) -> Result<AccountId> {
        if let Some(id) = self.link(kind, STAND_IN)? {
            return Ok(id);
        }
        let id = match self.accounts_in(Some(Scope::Household))?.into_iter().find(|a| !a.archived && find(a)) {
            Some(a) => a.id,
            None => self.create_account(&NewAccount::new(name, subtype).scope(Scope::Household))?,
        };
        self.remember(kind, STAND_IN, id)?;
        Ok(id)
    }

    /// Where Ledger's savings go: an existing household savings account, or
    /// a new one. Only made when a goal or a "Savings" sector needs it.
    fn ledger_savings_account(&self) -> Result<AccountId> {
        self.household_account("savings", "Savings", AccountSubtype::Bank, |a| a.kind() == AccountKind::Asset && a.name.to_lowercase().contains("sav"))
    }

    /// Counts what's in a Ledger database without bringing anything in.
    pub fn preview_ledger(path: &Path) -> Result<LedgerPreview> {
        let src = Source::open(path)?;
        let entries = src.entries()?;
        Ok(LedgerPreview {
            people: src.count("person")?,
            entries: entries.len(),
            months: src.count("month")?,
            goals: src.count("goal")?,
            bills: src.count("recurring")?,
            stores: src.count("store")?,
            first: entries.first().map(|e| e.date),
            last: entries.last().map(|e| e.date),
        })
    }

    /// Brings a Ledger database into this book's household side.
    /// `money_account` is where the money moved; a stand-in is made if absent.
    ///
    /// The steps run in this order because each points at what the earlier
    /// ones brought in: entries need people, sectors and goals; bills and
    /// rules need people and sectors.
    pub fn import_ledger(&mut self, path: &Path, money_account: Option<AccountId>) -> Result<LedgerImport> {
        // All of it or none of it, so an error never leaves half an import.
        self.atomically(|s| s.import_ledger_steps(path, money_account))
    }

    fn import_ledger_steps(&mut self, path: &Path, money_account: Option<AccountId>) -> Result<LedgerImport> {
        if !self.sides()?.contains(&Scope::Household) {
            return rule("add a household side to this book first (Settings → Business & household)");
        }
        let src = Source::open(path)?;
        let mut report = LedgerImport::default();
        let people = self.import_ledger_people(&src, &mut report)?;
        let money = self.ledger_money_account(money_account)?;
        report.money_account_id = money;
        let sectors = self.import_ledger_sectors(&src, &mut report)?;
        let goals = self.import_ledger_goals(&src, &mut report)?;
        let stores = read_stores(&src)?;
        self.import_ledger_entries(&src, money, &people, &sectors, &goals, &stores, &mut report)?;
        self.import_ledger_months(&src, &people, &mut report)?;
        self.import_ledger_bills(&src, &people, &sectors, &mut report)?;
        self.import_ledger_store_rules(&stores, &people, &sectors, &mut report)?;
        Ok(report)
    }

    /// People stay people. Returns Ledger's person ids mapped to ours.
    fn import_ledger_people(&self, src: &Source, report: &mut LedgerImport) -> Result<HashMap<String, PersonId>> {
        let mut people: HashMap<String, PersonId> = HashMap::new();
        let mut stmt = src.conn.prepare("SELECT id, name, archived_at FROM person ORDER BY sort_order, id")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        for (id, name, archived) in rows {
            let local = match self.link("person", &id)? {
                Some(p) => p,
                None => {
                    let p = self.create_person(&NewPerson { name, color: None })?;
                    if archived.is_some() {
                        self.set_person_archived(p, true)?;
                    }
                    self.remember("person", &id, p)?;
                    report.people += 1;
                    p
                }
            };
            people.insert(id, local);
        }
        Ok(people)
    }

    /// The account every imported entry moves money through: the one the
    /// user chose, or a stand-in, since Ledger never said which bank paid.
    fn ledger_money_account(&self, chosen: Option<AccountId>) -> Result<AccountId> {
        match chosen {
            Some(id) => {
                let a = self.account(id)?;
                if a.scope != Scope::Household || a.kind() != AccountKind::Asset {
                    return rule("choose a household bank or cash account for the imported money");
                }
                self.remember("money", STAND_IN, id)?;
                Ok(id)
            }
            None => self.household_account("money", "Ledger import", AccountSubtype::OtherCurrentAsset, |a| a.name == "Ledger import"),
        }
    }

    /// Sectors become household categories: one with the same name, else
    /// the chart's match (`SECTOR_CODES`), else a new one. A "Savings" sector
    /// is the savings account. Returns Ledger's sector ids mapped to accounts.
    fn import_ledger_sectors(&self, src: &Source, report: &mut LedgerImport) -> Result<HashMap<String, AccountId>> {
        let mut sectors: HashMap<String, AccountId> = HashMap::new();
        if !src.has_table("sector")? {
            return Ok(sectors);
        }
        let change = if src.has_column("sector", "changeability")? { "changeability" } else { "'medium'" };
        let mut stmt = src.conn.prepare(&format!("SELECT id, name, {change} FROM sector"))?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let household = self.accounts_in(Some(Scope::Household))?;
        for (id, name, changeability) in rows {
            let local = if let Some(a) = self.link("sector", &id)? {
                a
            } else {
                let key = name.trim().to_lowercase();
                let a = if key == "savings" {
                    self.ledger_savings_account()?
                } else if let Some(a) = household.iter().find(|a| a.kind() == AccountKind::Expense && a.name.to_lowercase() == key && !a.archived) {
                    a.id
                } else if let Some((_, code)) = SECTOR_CODES.iter().find(|(n, _)| *n == key) {
                    self.code_id(code)?
                } else {
                    report.categories_created += 1;
                    self.create_account(&NewAccount {
                        code: None,
                        name: name.trim().to_string(),
                        subtype: AccountSubtype::Expense,
                        parent_id: None,
                        scope: Scope::Household,
                        flex: Some(flex_of(&changeability)),
                    })?
                };
                self.remember("sector", &id, a)?;
                a
            };
            sectors.insert(id, local);
        }
        Ok(sectors)
    }

    /// Goals keep their money in the savings account. Returns Ledger's goal
    /// ids mapped to (goal, its account).
    fn import_ledger_goals(&self, src: &Source, report: &mut LedgerImport) -> Result<HashMap<String, (crate::GoalId, AccountId)>> {
        let mut goals: HashMap<String, (crate::GoalId, AccountId)> = HashMap::new();
        if !src.has_table("goal")? {
            return Ok(goals);
        }
        let mut stmt = src.conn.prepare("SELECT id, name, target_cents, target_date, note, archived_at FROM goal")?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, Option<String>>(3)?, r.get::<_, String>(4)?, r.get::<_, Option<String>>(5)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (id, name, target, date, note, archived) in rows {
            let account = self.ledger_savings_account()?;
            let local = match self.link("goal", &id)? {
                Some(g) => g,
                None => {
                    let g = self.create_goal(&NewGoal {
                        name,
                        // A goal here needs something to reach; Ledger allowed zero.
                        target: Cents(target.max(1)),
                        target_date: date.and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok()),
                        account_id: account,
                        starting: Cents::ZERO,
                        note,
                    })?;
                    if archived.is_some() {
                        self.set_goal_archived(g, true)?;
                    }
                    self.remember("goal", &id, g)?;
                    report.goals += 1;
                    g
                }
            };
            goals.insert(id, (local, account));
        }
        Ok(goals)
    }

    /// Each Ledger entry becomes a balanced two-line entry between the money
    /// account and where the money went (a category, or a goal's account).
    #[allow(clippy::too_many_arguments)]
    fn import_ledger_entries(
        &mut self,
        src: &Source,
        money: AccountId,
        people: &HashMap<String, PersonId>,
        sectors: &HashMap<String, AccountId>,
        goals: &HashMap<String, (crate::GoalId, AccountId)>,
        stores: &HashMap<String, LedgerStore>,
        report: &mut LedgerImport,
    ) -> Result<()> {
        let uncategorized = self.code_id(codes::HOUSEHOLD_UNCATEGORIZED_EXPENSE)?;
        let other_income = self.code_id(codes::HOUSEHOLD_UNCATEGORIZED_INCOME)?;
        for e in src.entries()? {
            if self.link("entry", &e.id)?.is_some() {
                report.entries_skipped += 1;
                continue;
            }
            if e.amount == 0 {
                continue;
            }
            let store_name = e.store.as_ref().and_then(|s| stores.get(s)).map(|s| s.name.clone());
            let memo = match (store_name, e.note.trim()) {
                (Some(s), "") => s,
                (Some(s), n) => format!("{s} · {n}"),
                (None, "") => "Ledger entry".to_string(),
                (None, n) => n.to_string(),
            };
            let amount = Cents(e.amount);
            let goal = e.goal.as_ref().and_then(|g| goals.get(g)).copied();
            // The other side of the money account: where it went, or came from.
            let other = match (goal, e.sector.as_ref().and_then(|s| sectors.get(s))) {
                (Some((_, account)), _) => account,
                (None, Some(a)) => *a,
                (None, None) if amount.0 < 0 => uncategorized,
                (None, None) => other_income,
            };
            // Ledger counts money moved into a goal as positive; here it's a
            // transfer from the money account into the goal's account.
            let into_other = if goal.is_some() { amount } else { -amount };
            let entry = NewEntry::new(e.date, memo)
                .line(EntryLine { account_id: other, amount: into_other, memo: None })
                .line(EntryLine { account_id: money, amount: -into_other, memo: None });
            let id = self.post_entry_from(&entry, ENTRY_SOURCE)?;
            if let Some(p) = e.person.as_ref().and_then(|p| people.get(p)) {
                self.set_entry_person(id, Some(*p))?;
            }
            if let Some((g, _)) = goal {
                self.conn.execute("UPDATE journal_entries SET goal_id = ?1 WHERE id = ?2", params![g, id])?;
            }
            self.remember("entry", &e.id, id)?;
            report.entries += 1;
        }
        for (g, _) in goals.values() {
            self.refresh_goal(*g)?;
        }
        Ok(())
    }

    /// Ledger's months: what there was to work with, and each person's
    /// slice, as person budgets. A month that already has a plan here keeps it.
    fn import_ledger_months(&mut self, src: &Source, people: &HashMap<String, PersonId>, report: &mut LedgerImport) -> Result<()> {
        let mut floors_skipped = 0;
        let mut stmt = src.conn.prepare("SELECT id, allocated_cents FROM month ORDER BY id")?;
        let months = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut alloc = src.conn.prepare("SELECT person_id, amount_cents, direction FROM allocation WHERE month_id = ?1")?;
        for (month, allocated) in months {
            if crate::month_range(&month).is_err() || self.link("month", &month)?.is_some() {
                continue;
            }
            let own: bool = self.conn.query_row("SELECT COUNT(*) FROM budget_months WHERE month = ?1", [&month], |r| r.get::<_, i64>(0))? > 0;
            if own {
                report.notes.push(format!("{month} already had a plan here, so Ledger's plan for it was left out."));
                continue;
            }
            let lines = alloc
                .query_map([&month], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let tx = self.conn.savepoint()?;
            tx.execute("INSERT INTO budget_months(month, available) VALUES (?1, ?2)", params![month, (allocated > 0).then_some(allocated)])?;
            for (person, amount, direction) in lines {
                let Some(p) = people.get(&person) else { continue };
                if direction == "floor" {
                    floors_skipped += 1;
                    continue;
                }
                if amount > 0 {
                    insert_with_id(&tx, "budgets", "INSERT INTO budgets(month, person_id, amount, id) VALUES (?1, ?2, ?3, ?4)", params![month, p, amount])?;
                }
            }
            tx.commit()?;
            // A month has no row id of its own here; the link only says it's done.
            self.remember("month", &month, MONTH_LINK)?;
            report.months += 1;
        }
        if floors_skipped > 0 {
            report.notes.push(format!(
                "{floors_skipped} \"at least\" amounts for people weren't brought over: here, savings targets belong on a savings account. Add them in a month's plan."
            ));
        }
        Ok(())
    }

    /// Ledger's recurring items become bills, starting from the month of the
    /// first entry so their history can tick off.
    fn import_ledger_bills(&self, src: &Source, people: &HashMap<String, PersonId>, sectors: &HashMap<String, AccountId>, report: &mut LedgerImport) -> Result<()> {
        if !src.has_table("recurring")? {
            return Ok(());
        }
        let uncategorized = self.code_id(codes::HOUSEHOLD_UNCATEGORIZED_EXPENSE)?;
        let sector_col = if src.has_column("recurring", "sector_id")? { "sector_id" } else { "NULL" };
        let auto_col = if src.has_column("recurring", "auto_debit")? { "auto_debit" } else { "0" };
        let mut stmt = src.conn.prepare(&format!(
            "SELECT id, person_id, label, amount_cents, cadence, day_of_week, day_of_month, months_interval, {sector_col}, {auto_col}, active FROM recurring"
        ))?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<u32>>(5)?,
                    r.get::<_, Option<u32>>(6)?,
                    r.get::<_, Option<u32>>(7)?,
                    r.get::<_, Option<String>>(8)?,
                    r.get::<_, bool>(9)?,
                    r.get::<_, bool>(10)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // With no entries there's no history to start from; any early date does.
        let first = src.entries()?.first().map(|e| e.date).unwrap_or_else(|| NaiveDate::from_ymd_opt(2024, 1, 1).expect("valid"));
        for (id, person, label, amount, cadence, dow, dom, interval, sector, auto, active) in rows {
            if self.link("bill", &id)?.is_some() {
                continue;
            }
            let category = sector
                .as_ref()
                .and_then(|s| sectors.get(s))
                .copied()
                .filter(|a| self.account(*a).is_ok_and(|x| x.kind() == AccountKind::Expense))
                .unwrap_or(uncategorized);
            let start = NaiveDate::from_ymd_opt(first.year(), first.month(), 1).expect("valid");
            // Out-of-range days and intervals are clamped to what `Cadence` accepts.
            let (cadence, start) = match cadence.as_str() {
                "weekly" => {
                    // Ledger counts weekdays from Sunday = 0.
                    let want = dow.unwrap_or(0) % 7;
                    let mut d = start;
                    while d.weekday().num_days_from_sunday() != want {
                        d = d + Days::new(1);
                    }
                    (Cadence::Week, d)
                }
                "every_n_months" => (Cadence::Months { months: interval.unwrap_or(1).clamp(1, 24), day: dom.unwrap_or(1).clamp(1, 31) }, start),
                _ => (Cadence::Month { day: dom.unwrap_or(1).clamp(1, 31) }, start),
            };
            let bill = self.create_recurring_bill(&NewRecurringBill {
                name: label,
                // i64::MIN has no positive; saturated, it's refused as too big like any absurd amount.
                amount: Cents(amount.saturating_abs()),
                varies: false,
                account_id: category,
                paid_from_id: None,
                person_id: person.as_ref().and_then(|p| people.get(p)).copied(),
                cadence,
                start_date: start,
                auto_pay: auto,
                match_text: None,
            })?;
            if !active {
                self.set_recurring_bill_active(bill, false)?;
            }
            self.remember("bill", &id, bill)?;
            report.bills += 1;
        }
        Ok(())
    }

    /// Stores become rules, so bank imports land already sorted. Archived
    /// stores, very short names (they'd match too much), and names a rule
    /// already uses are left out.
    fn import_ledger_store_rules(
        &self,
        stores: &HashMap<String, LedgerStore>,
        people: &HashMap<String, PersonId>,
        sectors: &HashMap<String, AccountId>,
        report: &mut LedgerImport,
    ) -> Result<()> {
        let existing: Vec<String> = self.rules()?.into_iter().map(|r| r.pattern.to_lowercase()).collect();
        for (id, store) in stores {
            let name = store.name.trim();
            if store.archived || self.link("store", id)?.is_some() || name.len() < MIN_RULE_PATTERN_LEN || existing.contains(&name.to_lowercase()) {
                continue;
            }
            let Some(target) = store.sector.as_ref().and_then(|s| sectors.get(s)).copied() else { continue };
            let r = self.create_rule(&NewRule {
                name: name.to_string(),
                priority: RULE_PRIORITY,
                op: MatchOp::Contains,
                pattern: name.to_string(),
                direction: RuleDirection::Outflow,
                amount_min: None,
                amount_max: None,
                bank_account_id: None,
                target_account_id: target,
                rename_to: None,
                auto_post: false,
                person_id: store.person.as_ref().and_then(|p| people.get(p)).copied(),
            })?;
            self.remember("store", id, r)?;
            report.rules += 1;
        }
        Ok(())
    }
}
