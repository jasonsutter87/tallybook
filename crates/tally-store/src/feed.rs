//! Bank-feed persistence and the "For Review" inbox workflow.
//!
//! Feed transactions land in `bank_transactions` with status `new`. From the
//! inbox the user (or an auto-post rule) either:
//! * **posts** it — a new journal entry against a category,
//! * **matches** it — links it to a line already in the register,
//! * **excludes** it — duplicates, personal spending, etc.
//!
//! Posting or matching marks the bank line `cleared`, so it shows up
//! pre-checked at reconciliation time. Every action can be undone.

use crate::{Result, StoreError, Store, check_entry_not_locked, check_not_locked, conversion_err, BadValue, entry_in, post_in, rule};
use chrono::NaiveDate;
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use tally_core::bank::{FeedBatch, FeedTransaction, normalize_description};
use tally_core::journal::EntryId;
use tally_core::rules::{self as rules_mod, Direction, MatchOp, Rule, RuleId};
use tally_core::{AccountId, Cents, EntryLine, NewEntry};

pub type ConnectionId = i64;
pub type LinkId = i64;
pub type BankTxnId = i64;

/// How far apart (days) a feed transaction and a register line can be and still match.
const MATCH_WINDOW_DAYS: i64 = 10;

// ---------------------------------------------------------------- types

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Plaid,
    Simplefin,
    File,
}

impl Provider {
    fn as_str(self) -> &'static str {
        match self {
            Provider::Plaid => "plaid",
            Provider::Simplefin => "simplefin",
            Provider::File => "file",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatus {
    Ok,
    NeedsRelink,
    Error,
    Disconnected,
}

impl ConnectionStatus {
    fn as_str(self) -> &'static str {
        match self {
            ConnectionStatus::Ok => "ok",
            ConnectionStatus::NeedsRelink => "needs_relink",
            ConnectionStatus::Error => "error",
            ConnectionStatus::Disconnected => "disconnected",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BankConnection {
    pub id: ConnectionId,
    pub provider: Provider,
    pub institution: String,
    pub external_id: Option<String>,
    pub secret_ref: Option<String>,
    pub cursor: String,
    pub status: ConnectionStatus,
    pub last_error: Option<String>,
    pub last_synced_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BankLink {
    pub id: LinkId,
    pub connection_id: ConnectionId,
    pub external_account_id: String,
    pub ledger_account_id: AccountId,
    pub name: String,
    pub mask: Option<String>,
    pub bank_balance: Option<Cents>,
    pub start_date: NaiveDate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BankTxnStatus {
    New,
    Posted,
    Matched,
    Excluded,
    Removed,
}

impl BankTxnStatus {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "new" => BankTxnStatus::New,
            "posted" => BankTxnStatus::Posted,
            "matched" => BankTxnStatus::Matched,
            "excluded" => BankTxnStatus::Excluded,
            "removed" => BankTxnStatus::Removed,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BankTxn {
    pub id: BankTxnId,
    pub ledger_account_id: AccountId,
    pub external_id: String,
    pub date: NaiveDate,
    /// Inflow-positive.
    pub amount: Cents,
    pub description: String,
    pub merchant: Option<String>,
    pub category_hint: Option<String>,
    pub pending: bool,
    pub status: BankTxnStatus,
    pub entry_id: Option<EntryId>,
    pub line_id: Option<crate::LineId>,
    pub rule_id: Option<RuleId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Suggestion {
    /// A register line that's probably this same transaction.
    Match { line_id: crate::LineId, entry_id: EntryId, date: NaiveDate, memo: String },
    /// A category from a rule.
    Rule { rule_id: RuleId, account_id: AccountId, rename_to: Option<String> },
    /// The category used last time for the same payee.
    History { account_id: AccountId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InboxItem {
    #[serde(flatten)]
    pub txn: BankTxn,
    pub suggestion: Option<Suggestion>,
    /// Who it was probably for: the rule's person, or the last one used for this payee.
    pub person_id: Option<crate::PersonId>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ImportSummary {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    /// Already imported, or dated before the account's start date.
    pub skipped: usize,
    /// Added and immediately posted by an auto-post rule.
    pub auto_posted: usize,
}

/// One piece of a split. Amounts share the bank transaction's (inflow-positive)
/// sign and must sum to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Split {
    pub account_id: AccountId,
    pub amount: Cents,
    pub memo: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewRule {
    pub name: String,
    pub priority: i64,
    pub op: MatchOp,
    pub pattern: String,
    pub direction: Direction,
    pub amount_min: Option<Cents>,
    pub amount_max: Option<Cents>,
    pub bank_account_id: Option<AccountId>,
    pub target_account_id: AccountId,
    pub rename_to: Option<String>,
    pub auto_post: bool,
    /// Who matching money is for (household rules); tagged on auto-posted
    /// entries and offered in Review.
    #[serde(default)]
    pub person_id: Option<crate::PersonId>,
}

// ---------------------------------------------------------------- row mapping

const CONNECTION_COLS: &str = "id, provider, institution, external_id, secret_ref, cursor, status, last_error, last_synced_at";

fn connection_from_row(r: &Row) -> rusqlite::Result<BankConnection> {
    let provider: String = r.get(1)?;
    let status: String = r.get(6)?;
    Ok(BankConnection {
        id: r.get(0)?,
        provider: match provider.as_str() {
            "plaid" => Provider::Plaid,
            "simplefin" => Provider::Simplefin,
            "file" => Provider::File,
            _ => return Err(conversion_err(1, BadValue(provider))),
        },
        institution: r.get(2)?,
        external_id: r.get(3)?,
        secret_ref: r.get(4)?,
        cursor: r.get(5)?,
        status: match status.as_str() {
            "ok" => ConnectionStatus::Ok,
            "needs_relink" => ConnectionStatus::NeedsRelink,
            "error" => ConnectionStatus::Error,
            "disconnected" => ConnectionStatus::Disconnected,
            _ => return Err(conversion_err(6, BadValue(status))),
        },
        last_error: r.get(7)?,
        last_synced_at: r.get(8)?,
    })
}

const LINK_COLS: &str = "id, connection_id, external_account_id, ledger_account_id, name, mask, bank_balance, start_date";

fn link_from_row(r: &Row) -> rusqlite::Result<BankLink> {
    Ok(BankLink {
        id: r.get(0)?,
        connection_id: r.get(1)?,
        external_account_id: r.get(2)?,
        ledger_account_id: r.get(3)?,
        name: r.get(4)?,
        mask: r.get(5)?,
        bank_balance: r.get::<_, Option<i64>>(6)?.map(Cents),
        start_date: r.get(7)?,
    })
}

const TXN_COLS: &str = "id, ledger_account_id, external_id, date, amount, description, merchant, category_hint, \
                        pending, status, entry_id, line_id, rule_id";

fn txn_from_row(r: &Row) -> rusqlite::Result<BankTxn> {
    let status: String = r.get(9)?;
    Ok(BankTxn {
        id: r.get(0)?,
        ledger_account_id: r.get(1)?,
        external_id: r.get(2)?,
        date: r.get(3)?,
        amount: Cents(r.get(4)?),
        description: r.get(5)?,
        merchant: r.get(6)?,
        category_hint: r.get(7)?,
        pending: r.get(8)?,
        status: BankTxnStatus::parse(&status).ok_or_else(|| conversion_err(9, BadValue(status)))?,
        entry_id: r.get(10)?,
        line_id: r.get(11)?,
        rule_id: r.get(12)?,
    })
}

const RULE_COLS: &str = "id, name, priority, op, pattern, direction, amount_min, amount_max, bank_account_id, \
                         target_account_id, rename_to, auto_post, person_id";

fn rule_from_row(r: &Row) -> rusqlite::Result<Rule> {
    let op: String = r.get(3)?;
    let direction: String = r.get(5)?;
    Ok(Rule {
        id: r.get(0)?,
        name: r.get(1)?,
        priority: r.get(2)?,
        op: match op.as_str() {
            "contains" => MatchOp::Contains,
            "equals" => MatchOp::Equals,
            "starts_with" => MatchOp::StartsWith,
            _ => return Err(conversion_err(3, BadValue(op))),
        },
        pattern: r.get(4)?,
        direction: match direction.as_str() {
            "any" => Direction::Any,
            "inflow" => Direction::Inflow,
            "outflow" => Direction::Outflow,
            _ => return Err(conversion_err(5, BadValue(direction))),
        },
        amount_min: r.get::<_, Option<i64>>(6)?.map(Cents),
        amount_max: r.get::<_, Option<i64>>(7)?.map(Cents),
        bank_account_id: r.get(8)?,
        target_account_id: r.get(9)?,
        rename_to: r.get(10)?,
        auto_post: r.get(11)?,
        person_id: r.get(12)?,
    })
}

fn op_str(op: MatchOp) -> &'static str {
    match op {
        MatchOp::Contains => "contains",
        MatchOp::Equals => "equals",
        MatchOp::StartsWith => "starts_with",
    }
}

fn direction_str(d: Direction) -> &'static str {
    match d {
        Direction::Any => "any",
        Direction::Inflow => "inflow",
        Direction::Outflow => "outflow",
    }
}

// ---------------------------------------------------------------- store

impl Store {
    // ------------------------------------------------------------ connections & links

    pub fn create_connection(
        &self,
        provider: Provider,
        institution: &str,
        external_id: Option<&str>,
        secret_ref: Option<&str>,
    ) -> Result<ConnectionId> {
        self.conn.execute(
            "INSERT INTO bank_connections(provider, institution, external_id, secret_ref) VALUES (?1, ?2, ?3, ?4)",
            params![provider.as_str(), institution.trim(), external_id, secret_ref],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn connections(&self) -> Result<Vec<BankConnection>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {CONNECTION_COLS} FROM bank_connections WHERE status <> 'disconnected' ORDER BY id"
        ))?;
        Ok(stmt.query_map([], connection_from_row)?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn connection(&self, id: ConnectionId) -> Result<BankConnection> {
        self.conn
            .query_row(&format!("SELECT {CONNECTION_COLS} FROM bank_connections WHERE id = ?1"), [id], connection_from_row)
            .optional()?
            .ok_or(StoreError::NotFound("bank connection", id))
    }

    /// Records a successful sync and its new cursor.
    pub fn record_sync(&self, id: ConnectionId, cursor: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE bank_connections SET cursor = ?1, status = 'ok', last_error = NULL,
                    last_synced_at = datetime('now') WHERE id = ?2",
            params![cursor, id],
        )?;
        Ok(())
    }

    pub fn set_connection_status(&self, id: ConnectionId, status: ConnectionStatus, error: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE bank_connections SET status = ?1, last_error = ?2 WHERE id = ?3",
            params![status.as_str(), error, id],
        )?;
        Ok(())
    }

    /// Every connection that still holds credentials, for teardown.
    pub fn connections_with_secrets(&self) -> Result<Vec<BankConnection>> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {CONNECTION_COLS} FROM bank_connections WHERE secret_ref IS NOT NULL ORDER BY id"))?;
        Ok(stmt.query_map([], connection_from_row)?.collect::<rusqlite::Result<_>>()?)
    }

    /// Unlinks every account and marks the connection disconnected. Imported
    /// transactions and posted entries are kept.
    pub fn disconnect(&mut self, id: ConnectionId) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM bank_links WHERE connection_id = ?1", [id])?;
        tx.execute("UPDATE bank_connections SET status = 'disconnected', secret_ref = NULL WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(())
    }

    /// Wires a provider account to a ledger account. Feed transactions dated
    /// before `start_date` are ignored (they belong in the opening balance).
    pub fn link_account(
        &self,
        connection_id: ConnectionId,
        external_account_id: &str,
        ledger_account_id: AccountId,
        name: &str,
        mask: Option<&str>,
        start_date: NaiveDate,
    ) -> Result<LinkId> {
        self.connection(connection_id)?;
        let account = self.account(ledger_account_id)?;
        if !account.subtype.is_reconcilable() {
            return rule(format!("{} isn't a bank, card, or loan account", account.name));
        }
        if self.link_for_account(ledger_account_id)?.is_some() {
            return rule(format!("{} is already linked to a bank feed", account.name));
        }
        self.conn.execute(
            "INSERT INTO bank_links(connection_id, external_account_id, ledger_account_id, name, mask, start_date)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![connection_id, external_account_id, ledger_account_id, name, mask, start_date],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn unlink_account(&self, link_id: LinkId) -> Result<()> {
        if self.conn.execute("DELETE FROM bank_links WHERE id = ?1", [link_id])? == 0 {
            return Err(StoreError::NotFound("bank link", link_id));
        }
        Ok(())
    }

    pub fn links(&self) -> Result<Vec<BankLink>> {
        let mut stmt = self.conn.prepare(&format!("SELECT {LINK_COLS} FROM bank_links ORDER BY id"))?;
        Ok(stmt.query_map([], link_from_row)?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn link_for_account(&self, ledger_account_id: AccountId) -> Result<Option<BankLink>> {
        Ok(self
            .conn
            .query_row(&format!("SELECT {LINK_COLS} FROM bank_links WHERE ledger_account_id = ?1"), [ledger_account_id], link_from_row)
            .optional()?)
    }

    pub fn link_for_external(&self, connection_id: ConnectionId, external_account_id: &str) -> Result<Option<BankLink>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {LINK_COLS} FROM bank_links WHERE connection_id = ?1 AND external_account_id = ?2"),
                params![connection_id, external_account_id],
                link_from_row,
            )
            .optional()?)
    }

    /// Stores what the bank says the balance is, for "bank vs. books" displays.
    pub fn set_link_balance(&self, link_id: LinkId, balance: Option<Cents>) -> Result<()> {
        self.conn.execute("UPDATE bank_links SET bank_balance = ?1 WHERE id = ?2", params![balance.map(|b| b.0), link_id])?;
        Ok(())
    }

    // ------------------------------------------------------------ import

    /// Applies a batch from any provider to one ledger account. Idempotent:
    /// re-importing the same transactions changes nothing.
    pub fn import_feed(&mut self, ledger_account_id: AccountId, batch: &FeedBatch) -> Result<ImportSummary> {
        let account = self.account(ledger_account_id)?;
        if !account.subtype.is_reconcilable() {
            return rule(format!("{} can't receive bank transactions", account.name));
        }
        let start = self.link_for_account(ledger_account_id)?.map(|l| l.start_date);
        let mut summary = ImportSummary::default();
        let mut fresh: Vec<BankTxnId> = Vec::new();

        let tx = self.conn.transaction()?;
        for t in batch.added.iter().chain(&batch.modified) {
            if start.is_some_and(|s| t.date < s) {
                summary.skipped += 1;
                continue;
            }
            let existing: Option<(BankTxnId, String)> = tx
                .query_row(
                    "SELECT id, status FROM bank_transactions WHERE ledger_account_id = ?1 AND external_id = ?2",
                    params![ledger_account_id, t.external_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            match existing {
                None => {
                    insert_txn(&tx, ledger_account_id, t)?;
                    fresh.push(tx.last_insert_rowid());
                    summary.added += 1;
                }
                // Only unreviewed rows follow the bank's edits; once the user
                // has acted, the books are the source of truth.
                Some((id, status)) if status == "new" || status == "removed" => {
                    tx.execute(
                        "UPDATE bank_transactions SET date = ?1, amount = ?2, description = ?3, normalized = ?4,
                                merchant = ?5, category_hint = ?6, pending = ?7, status = 'new' WHERE id = ?8",
                        params![
                            t.date,
                            t.amount.0,
                            t.description,
                            normalize_description(&t.description),
                            t.merchant,
                            t.category_hint,
                            t.pending,
                            id
                        ],
                    )?;
                    summary.updated += 1;
                }
                Some(_) => summary.skipped += 1,
            }
        }
        for external_id in &batch.removed {
            summary.removed += tx.execute(
                "UPDATE bank_transactions SET status = 'removed'
                 WHERE ledger_account_id = ?1 AND external_id = ?2 AND status = 'new'",
                params![ledger_account_id, external_id],
            )?;
        }
        tx.commit()?;

        // Auto-post rules run after the import commits, one entry per transaction.
        let rules = self.rules()?;
        for id in fresh {
            let t = self.bank_txn(id)?;
            if t.pending {
                continue;
            }
            if let Some(r) = rules_mod::first_match(&rules, ledger_account_id, &t.description, t.amount)
                && r.auto_post
                && let Ok(entry) = self.post_bank_txn_inner(&t, &[Split { account_id: r.target_account_id, amount: t.amount, memo: None }], r.rename_to.clone(), Some(r.id))
            {
                if r.person_id.is_some() {
                    self.set_entry_person(entry, r.person_id)?;
                }
                summary.auto_posted += 1;
            }
        }
        Ok(summary)
    }

    pub fn bank_txn(&self, id: BankTxnId) -> Result<BankTxn> {
        self.conn
            .query_row(&format!("SELECT {TXN_COLS} FROM bank_transactions WHERE id = ?1"), [id], txn_from_row)
            .optional()?
            .ok_or(StoreError::NotFound("bank transaction", id))
    }

    /// Transactions awaiting review, newest first, each with its best suggestion.
    pub fn inbox(&self, ledger_account_id: Option<AccountId>) -> Result<Vec<InboxItem>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {TXN_COLS} FROM bank_transactions
             WHERE status = 'new' AND (?1 IS NULL OR ledger_account_id = ?1)
             ORDER BY date DESC, id DESC"
        ))?;
        let txns: Vec<BankTxn> = stmt.query_map([ledger_account_id], txn_from_row)?.collect::<rusqlite::Result<_>>()?;
        let rules = self.rules()?;
        txns.into_iter()
            .map(|txn| {
                let suggestion = self.suggest(&txn, &rules)?;
                let person_id = self.suggest_person(&txn, &rules)?;
                Ok(InboxItem { txn, suggestion, person_id })
            })
            .collect()
    }

    /// Reviewed transactions (posted/matched/excluded), newest first.
    pub fn reviewed(&self, ledger_account_id: Option<AccountId>, limit: u32) -> Result<Vec<BankTxn>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {TXN_COLS} FROM bank_transactions
             WHERE status IN ('posted', 'matched', 'excluded') AND (?1 IS NULL OR ledger_account_id = ?1)
             ORDER BY date DESC, id DESC LIMIT ?2"
        ))?;
        Ok(stmt.query_map(params![ledger_account_id, limit], txn_from_row)?.collect::<rusqlite::Result<_>>()?)
    }

    /// Settled money out of household accounts still waiting in Review,
    /// dated in `[from, to]`, newest first, at most `limit`. Bill detection
    /// reads this rather than `inbox`, which works out a suggestion for every
    /// row, money in and business side included.
    pub(crate) fn review_money_out(&self, from: NaiveDate, to: NaiveDate, limit: usize) -> Result<Vec<BankTxn>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {TXN_COLS} FROM bank_transactions
             WHERE status = 'new' AND pending = 0 AND amount < 0 AND date BETWEEN ?1 AND ?2
               AND ledger_account_id IN (SELECT id FROM accounts WHERE scope = 'household')
             ORDER BY date DESC, id DESC LIMIT ?3"
        ))?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        Ok(stmt.query_map(params![from, to, limit], txn_from_row)?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn inbox_count(&self) -> Result<i64> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM bank_transactions WHERE status = 'new' AND pending = 0", [], |r| r.get(0))?)
    }

    /// Where a Review row probably goes, best evidence first: a register line
    /// it matches, the first rule that fits, or the category last used for
    /// the same payee.
    pub(crate) fn suggest(&self, t: &BankTxn, rules: &[Rule]) -> Result<Option<Suggestion>> {
        if let Some(m) = self.match_candidates(t)?.into_iter().next() {
            return Ok(Some(m));
        }
        if let Some(r) = rules_mod::first_match(rules, t.ledger_account_id, &t.description, t.amount) {
            return Ok(Some(Suggestion::Rule { rule_id: r.id, account_id: r.target_account_id, rename_to: r.rename_to.clone() }));
        }
        // Last simple (two-line) posting for the same payee, same direction.
        let account_id: Option<AccountId> = self
            .conn
            .query_row(
                "SELECT l.account_id
                 FROM bank_transactions bt
                 JOIN journal_entries e ON e.id = bt.entry_id
                 JOIN journal_lines l ON l.entry_id = bt.entry_id AND l.id <> bt.line_id
                 JOIN accounts a ON a.id = l.account_id
                 WHERE bt.normalized = ?1 AND bt.status = 'posted' AND bt.id <> ?2
                   AND (bt.amount > 0) = (?3 > 0)
                   AND e.voided_by_id IS NULL AND a.archived = 0
                   AND (SELECT COUNT(*) FROM journal_lines x WHERE x.entry_id = bt.entry_id) = 2
                 ORDER BY bt.date DESC, bt.id DESC LIMIT 1",
                params![normalize_description(&t.description), t.id, t.amount.0],
                |r| r.get(0),
            )
            .optional()?;
        Ok(account_id.map(|account_id| Suggestion::History { account_id }))
    }

    /// Who a Review row was probably for: the first matching rule's person,
    /// else whoever the last posted payment from the same payee was tagged
    /// with (if they aren't archived). `None` when nothing says.
    pub(crate) fn suggest_person(&self, t: &BankTxn, rules: &[Rule]) -> Result<Option<crate::PersonId>> {
        if let Some(r) = rules_mod::first_match(rules, t.ledger_account_id, &t.description, t.amount)
            && r.person_id.is_some()
        {
            return Ok(r.person_id);
        }
        Ok(self
            .conn
            .query_row(
                "SELECT e.person_id
                 FROM bank_transactions bt
                 JOIN journal_entries e ON e.id = bt.entry_id
                 JOIN people p ON p.id = e.person_id
                 WHERE bt.normalized = ?1 AND bt.status = 'posted' AND bt.id <> ?2
                   AND e.voided_by_id IS NULL AND p.archived = 0
                 ORDER BY bt.date DESC, bt.id DESC LIMIT 1",
                params![normalize_description(&t.description), t.id],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Register lines that could be this transaction: same account, same
    /// amount, not yet tied to a bank transaction, close in date. Closest first.
    pub fn match_candidates(&self, t: &BankTxn) -> Result<Vec<Suggestion>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT l.id, e.id, e.date, COALESCE(l.memo, e.memo)
             FROM journal_lines l JOIN journal_entries e ON e.id = l.entry_id
             WHERE l.account_id = ?1 AND l.amount = ?2 AND l.cleared <> 'reconciled'
               AND e.voided_by_id IS NULL AND e.reverses_id IS NULL
               AND ABS(julianday(e.date) - julianday(?3)) <= ?4
               AND NOT EXISTS (SELECT 1 FROM bank_transactions bt WHERE bt.line_id = l.id)
             ORDER BY ABS(julianday(e.date) - julianday(?3)), e.id",
        )?;
        let rows = stmt
            .query_map(params![t.ledger_account_id, t.amount.0, t.date, MATCH_WINDOW_DAYS], |r| {
                Ok(Suggestion::Match { line_id: r.get(0)?, entry_id: r.get(1)?, date: r.get(2)?, memo: r.get(3)? })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    // ------------------------------------------------------------ review actions

    /// Posts the transaction to a single category.
    pub fn accept(&mut self, id: BankTxnId, category: AccountId, memo: Option<String>) -> Result<EntryId> {
        let t = self.bank_txn(id)?;
        self.post_bank_txn_inner(&t, &[Split { account_id: category, amount: t.amount, memo: None }], memo, None)
    }

    /// Posts the transaction across several categories.
    pub fn accept_split(&mut self, id: BankTxnId, splits: &[Split], memo: Option<String>) -> Result<EntryId> {
        let t = self.bank_txn(id)?;
        self.post_bank_txn_inner(&t, splits, memo, None)
    }

    fn post_bank_txn_inner(&mut self, t: &BankTxn, splits: &[Split], memo: Option<String>, rule_id: Option<RuleId>) -> Result<EntryId> {
        if t.status != BankTxnStatus::New {
            return rule("this transaction has already been reviewed");
        }
        if t.pending {
            return rule("pending transactions can't be posted until they clear");
        }
        if splits.is_empty() {
            return rule("choose a category");
        }
        let total: Cents = splits.iter().map(|s| s.amount).sum();
        if total != t.amount {
            return rule(format!("splits add up to {total} but the transaction is {}", t.amount));
        }
        if splits.iter().any(|s| s.account_id == t.ledger_account_id) {
            return rule("a transaction can't be categorized to its own bank account");
        }
        let memo = memo.filter(|m| !m.trim().is_empty()).unwrap_or_else(|| t.description.clone());
        let mut entry = NewEntry::new(t.date, memo).line(EntryLine { account_id: t.ledger_account_id, amount: t.amount, memo: None });
        for s in splits {
            entry = entry.line(EntryLine { account_id: s.account_id, amount: -s.amount, memo: s.memo.clone() });
        }

        let tx = self.conn.transaction()?;
        let entry_id = post_in(&tx, &entry, "bank_feed", None)?;
        let bank_line: crate::LineId =
            tx.query_row("SELECT MIN(id) FROM journal_lines WHERE entry_id = ?1", [entry_id], |r| r.get(0))?;
        tx.execute("UPDATE journal_lines SET cleared = 'cleared' WHERE id = ?1", [bank_line])?;
        tx.execute(
            "UPDATE bank_transactions SET status = 'posted', entry_id = ?1, line_id = ?2, rule_id = ?3 WHERE id = ?4",
            params![entry_id, bank_line, rule_id, t.id],
        )?;
        tx.commit()?;
        Ok(entry_id)
    }

    /// Links the transaction to an existing register line (e.g. a check you
    /// already entered) instead of creating a duplicate.
    pub fn match_txn(&mut self, id: BankTxnId, line_id: crate::LineId) -> Result<()> {
        let t = self.bank_txn(id)?;
        if t.status != BankTxnStatus::New {
            return rule("this transaction has already been reviewed");
        }
        let ok = self.match_candidates(&t)?.iter().any(|s| matches!(s, Suggestion::Match { line_id: l, .. } if *l == line_id));
        if !ok {
            return rule("that register line doesn't match this transaction's account, amount, and date");
        }
        let entry_id: EntryId = self.conn.query_row("SELECT entry_id FROM journal_lines WHERE id = ?1", [line_id], |r| r.get(0))?;
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE journal_lines SET cleared = 'cleared' WHERE id = ?1 AND cleared = 'uncleared'",
            [line_id],
        )?;
        tx.execute(
            "UPDATE bank_transactions SET status = 'matched', entry_id = ?1, line_id = ?2 WHERE id = ?3",
            params![entry_id, line_id, id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn exclude(&self, id: BankTxnId) -> Result<()> {
        let n = self.conn.execute("UPDATE bank_transactions SET status = 'excluded' WHERE id = ?1 AND status = 'new'", [id])?;
        if n == 0 {
            return rule("only unreviewed transactions can be excluded");
        }
        Ok(())
    }

    /// Sends a reviewed transaction back to the inbox. A posted entry is
    /// voided (same date), a match is unlinked.
    pub fn undo_review(&mut self, id: BankTxnId) -> Result<()> {
        let t = self.bank_txn(id)?;
        match t.status {
            BankTxnStatus::New | BankTxnStatus::Removed => return rule("this transaction hasn't been reviewed"),
            BankTxnStatus::Excluded => {}
            BankTxnStatus::Posted => {
                let entry_id = t.entry_id.ok_or_else(|| StoreError::Rule("posted transaction has no entry".into()))?;
                check_not_locked(&self.conn, t.date)?;
                let entry = entry_in(&self.conn, entry_id)?;
                if entry.voided_by_id.is_none() {
                    self.void_entry(entry_id, entry.date)?;
                }
            }
            BankTxnStatus::Matched => {
                let line_id = t.line_id.ok_or_else(|| StoreError::Rule("matched transaction has no line".into()))?;
                // Unmatching changes the matched entry's line, so its period must be open.
                let entry_id: EntryId = self.conn.query_row("SELECT entry_id FROM journal_lines WHERE id = ?1", [line_id], |r| r.get(0))?;
                check_entry_not_locked(&self.conn, entry_id)?;
                let cleared: String = self.conn.query_row("SELECT cleared FROM journal_lines WHERE id = ?1", [line_id], |r| r.get(0))?;
                if cleared == "reconciled" {
                    return rule("that line is reconciled; undo the reconciliation first");
                }
                self.conn.execute(
                    "UPDATE journal_lines SET cleared = 'uncleared', reconciliation_id = NULL WHERE id = ?1",
                    [line_id],
                )?;
            }
        }
        self.conn.execute(
            "UPDATE bank_transactions SET status = 'new', entry_id = NULL, line_id = NULL, rule_id = NULL WHERE id = ?1",
            [id],
        )?;
        Ok(())
    }

    // ------------------------------------------------------------ rules

    pub fn rules(&self) -> Result<Vec<Rule>> {
        let mut stmt = self.conn.prepare(&format!("SELECT {RULE_COLS} FROM rules ORDER BY priority, id"))?;
        Ok(stmt.query_map([], rule_from_row)?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn create_rule(&self, r: &NewRule) -> Result<RuleId> {
        validate_rule(self, r)?;
        self.conn.execute(
            &format!("INSERT INTO rules({}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)", &RULE_COLS[4..]),
            params![
                r.name.trim(),
                r.priority,
                op_str(r.op),
                r.pattern.trim(),
                direction_str(r.direction),
                r.amount_min.map(|c| c.0),
                r.amount_max.map(|c| c.0),
                r.bank_account_id,
                r.target_account_id,
                r.rename_to.as_deref().map(str::trim).filter(|s| !s.is_empty()),
                r.auto_post,
                r.person_id
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_rule(&self, id: RuleId, r: &NewRule) -> Result<()> {
        validate_rule(self, r)?;
        let n = self.conn.execute(
            "UPDATE rules SET name = ?1, priority = ?2, op = ?3, pattern = ?4, direction = ?5, amount_min = ?6,
                    amount_max = ?7, bank_account_id = ?8, target_account_id = ?9, rename_to = ?10, auto_post = ?11,
                    person_id = ?13
             WHERE id = ?12",
            params![
                r.name.trim(),
                r.priority,
                op_str(r.op),
                r.pattern.trim(),
                direction_str(r.direction),
                r.amount_min.map(|c| c.0),
                r.amount_max.map(|c| c.0),
                r.bank_account_id,
                r.target_account_id,
                r.rename_to.as_deref().map(str::trim).filter(|s| !s.is_empty()),
                r.auto_post,
                id,
                r.person_id
            ],
        )?;
        if n == 0 {
            return Err(StoreError::NotFound("rule", id));
        }
        Ok(())
    }

    pub fn delete_rule(&mut self, id: RuleId) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("UPDATE bank_transactions SET rule_id = NULL WHERE rule_id = ?1", [id])?;
        tx.execute("DELETE FROM rules WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(())
    }

    /// Posts an opening balance (natural sign) against Opening Balance Equity.
    pub fn post_opening_balance(&mut self, account_id: AccountId, date: NaiveDate, balance: Cents) -> Result<EntryId> {
        let account = self.account(account_id)?;
        let code = tally_core::chart::opening_balance_code(account.scope);
        let equity = self
            .account_by_code(code)?
            .ok_or_else(|| StoreError::Rule(format!("the opening balance account ({code}) is missing")))?;
        let raw = account.kind().raw(balance);
        if raw.is_zero() {
            return rule("opening balance is zero; nothing to post");
        }
        let entry = NewEntry::new(date, format!("Opening balance: {}", account.name))
            .line(EntryLine { account_id, amount: raw, memo: None })
            .line(EntryLine { account_id: equity.id, amount: -raw, memo: None });
        let tx = self.conn.transaction()?;
        let id = post_in(&tx, &entry, "opening_balance", None)?;
        // The bank already counted this money; pre-clear it for the first reconciliation.
        tx.execute("UPDATE journal_lines SET cleared = 'cleared' WHERE entry_id = ?1 AND account_id = ?2", params![id, account_id])?;
        tx.commit()?;
        Ok(id)
    }
}

fn validate_rule(store: &Store, r: &NewRule) -> Result<()> {
    if r.name.trim().is_empty() || r.pattern.trim().is_empty() {
        return rule("a rule needs a name and some text to match");
    }
    if let (Some(lo), Some(hi)) = (r.amount_min, r.amount_max)
        && lo > hi
    {
        return rule("minimum amount is greater than maximum");
    }
    let target = store.account(r.target_account_id)?;
    if target.archived {
        return rule(format!("{} is archived", target.name));
    }
    if let Some(p) = r.person_id
        && store.person(p)?.archived
    {
        return rule("that person is archived");
    }
    Ok(())
}

fn insert_txn(tx: &rusqlite::Transaction, ledger_account_id: AccountId, t: &FeedTransaction) -> Result<()> {
    tx.execute(
        "INSERT INTO bank_transactions(ledger_account_id, external_id, date, amount, description, normalized,
                                       merchant, category_hint, pending)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            ledger_account_id,
            t.external_id,
            t.date,
            t.amount.0,
            t.description,
            normalize_description(&t.description),
            t.merchant,
            t.category_hint,
            t.pending
        ],
    )?;
    Ok(())
}
