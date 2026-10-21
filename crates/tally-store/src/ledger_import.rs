//! Bringing a household over from Ledger (mels-budget-app).
//!
//! Ledger keeps a single-entry list: signed amounts, each tagged with who it
//! was for and what it was (a "sector"), with no notion of which bank paid.
//! Here every entry becomes a balanced entry against one stand-in money
//! account, people stay people, sectors become household categories, stores
//! become rules, monthly allocations become person budgets, recurring items
//! become bills, and goals keep their contributions.
//!
//! The source is opened read-only and never changed. Importing again adds
//! only what's new: every row brought in is remembered in `import_links`.

use crate::{NewAccount, NewGoal, NewPerson, NewRecurringBill, NewRule, PersonId, Result, Store, StoreError, rule};
use chrono::{Datelike, Days, NaiveDate};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use tally_core::chart::codes;
use tally_core::rules::{Direction as RuleDirection, MatchOp};
use tally_core::schedule::Cadence;
use tally_core::{AccountId, AccountKind, AccountSubtype, Cents, EntryLine, Flex, NewEntry, Scope};

const SOURCE: &str = "ledger";

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

struct Source {
    conn: Connection,
}

impl Source {
    fn open(path: &Path) -> Result<Source> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
            .map_err(|e| StoreError::Rule(format!("couldn't open {}: {e}", path.display())))?;
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

fn flex_of(changeability: &str) -> Flex {
    match changeability {
        "low" => Flex::Fixed,
        "high" => Flex::Lifestyle,
        _ => Flex::Usage,
    }
}

impl Store {
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

    /// Finds a household account of a kind by name, or makes one.
    fn household_account(&self, kind: &str, name: &str, subtype: AccountSubtype, find: impl Fn(&tally_core::Account) -> bool) -> Result<AccountId> {
        if let Some(id) = self.link(kind, "default")? {
            return Ok(id);
        }
        let id = match self.accounts_in(Some(Scope::Household))?.into_iter().find(|a| !a.archived && find(a)) {
            Some(a) => a.id,
            None => self.create_account(&NewAccount::new(name, subtype).scope(Scope::Household))?,
        };
        self.remember(kind, "default", id)?;
        Ok(id)
    }

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
    pub fn import_ledger(&mut self, path: &Path, money_account: Option<AccountId>) -> Result<LedgerImport> {
        if !self.sides()?.contains(&Scope::Household) {
            return rule("add a household side to this book first (Settings → Business & household)");
        }
        let src = Source::open(path)?;
        let mut report = LedgerImport::default();

        // ---- people
        let mut people: HashMap<String, PersonId> = HashMap::new();
        {
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
        }

        // ---- money and savings accounts
        let money = match money_account {
            Some(id) => {
                let a = self.account(id)?;
                if a.scope != Scope::Household || a.kind() != AccountKind::Asset {
                    return rule("choose a household bank or cash account for the imported money");
                }
                self.remember("money", "default", id)?;
                id
            }
            None => self.household_account("money", "Ledger import", AccountSubtype::OtherCurrentAsset, |a| a.name == "Ledger import")?,
        };
        report.money_account_id = money;
        let mut savings: Option<AccountId> = None;
        let mut savings_account = |s: &Store| -> Result<AccountId> {
            if let Some(id) = savings {
                return Ok(id);
            }
            let id = s.household_account("savings", "Savings", AccountSubtype::Bank, |a| a.kind() == AccountKind::Asset && a.name.to_lowercase().contains("sav"))?;
            savings = Some(id);
            Ok(id)
        };

        // ---- sectors → categories (or, for "Savings", the savings account)
        let mut sectors: HashMap<String, AccountId> = HashMap::new();
        if src.has_table("sector")? {
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
                        savings_account(self)?
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
        }

        // ---- goals
        let mut goals: HashMap<String, (crate::GoalId, AccountId)> = HashMap::new();
        if src.has_table("goal")? {
            let mut stmt = src.conn.prepare("SELECT id, name, target_cents, target_date, note, archived_at FROM goal")?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, Option<String>>(3)?, r.get::<_, String>(4)?, r.get::<_, Option<String>>(5)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (id, name, target, date, note, archived) in rows {
                let account = savings_account(self)?;
                let local = match self.link("goal", &id)? {
                    Some(g) => g,
                    None => {
                        let g = self.create_goal(&NewGoal {
                            name,
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
        }

        // ---- stores, for memos and rules
        let mut stores: HashMap<String, (String, Option<String>, Option<String>, bool)> = HashMap::new();
        if src.has_table("store")? {
            let mut stmt = src.conn.prepare("SELECT id, name, sector_id, default_person_id, archived_at FROM store")?;
            for row in stmt.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, Option<String>>(3)?, r.get::<_, Option<String>>(4)?))
            })? {
                let (id, name, sector, person, archived) = row?;
                stores.insert(id, (name, sector, person, archived.is_some()));
            }
        }

        // ---- entries
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
            let store_name = e.store.as_ref().and_then(|s| stores.get(s)).map(|s| s.0.clone());
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
            let id = self.post_entry_from(&entry, "ledger_import")?;
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

        // ---- months: what there was to work with, and each person's slice
        let mut floors_skipped = 0;
        {
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
                let tx = self.conn.transaction()?;
                tx.execute("INSERT INTO budget_months(month, available) VALUES (?1, ?2)", params![month, (allocated > 0).then_some(allocated)])?;
                for (person, amount, direction) in lines {
                    let Some(p) = people.get(&person) else { continue };
                    if direction == "floor" {
                        floors_skipped += 1;
                        continue;
                    }
                    if amount > 0 {
                        tx.execute("INSERT INTO budgets(month, person_id, amount) VALUES (?1, ?2, ?3)", params![month, p, amount])?;
                    }
                }
                tx.commit()?;
                self.remember("month", &month, 0)?;
                report.months += 1;
            }
        }
        if floors_skipped > 0 {
            report.notes.push(format!(
                "{floors_skipped} \"at least\" amounts for people weren't brought over: here, savings targets belong on a savings account. Add them in a month's plan."
            ));
        }

        // ---- recurring → bills
        if src.has_table("recurring")? {
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
                    amount: Cents(amount.abs()),
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
        }

        // ---- stores → rules, so bank imports land already sorted
        let existing: Vec<String> = self.rules()?.into_iter().map(|r| r.pattern.to_lowercase()).collect();
        for (id, (name, sector, person, archived)) in &stores {
            if *archived || self.link("store", id)?.is_some() || name.trim().len() < 3 || existing.contains(&name.trim().to_lowercase()) {
                continue;
            }
            let Some(target) = sector.as_ref().and_then(|s| sectors.get(s)).copied() else { continue };
            let r = self.create_rule(&NewRule {
                name: name.trim().to_string(),
                priority: 100,
                op: MatchOp::Contains,
                pattern: name.trim().to_string(),
                direction: RuleDirection::Outflow,
                amount_min: None,
                amount_max: None,
                bank_account_id: None,
                target_account_id: target,
                rename_to: None,
                auto_post: false,
                person_id: person.as_ref().and_then(|p| people.get(p)).copied(),
            })?;
            self.remember("store", id, r)?;
            report.rules += 1;
        }
        Ok(report)
    }
}
