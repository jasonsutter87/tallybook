//! Reconciling an account against a bank statement: starting a session,
//! ticking lines off, and finishing or undoing it.

use crate::{insert_with_id, BadValue, Cleared, LineId, ReconciliationId, Result, Store, StoreError, conversion_err, rule};
use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use tally_core::journal::EntryId;
use tally_core::reconcile::ReconcileSummary;
use tally_core::{AccountId, Cents};

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

impl Store {
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
        let tx = self.conn.savepoint()?;
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
        let id = insert_with_id(
            &tx,
            "reconciliations",
            "INSERT INTO reconciliations(account_id, statement_date, statement_balance, id) VALUES (?1, ?2, ?3, ?4)",
            params![account_id, statement_date, statement_balance.0],
        )?;
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
        let tx = self.conn.savepoint()?;
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
        let tx = self.conn.savepoint()?;
        tx.execute("UPDATE journal_lines SET reconciliation_id = NULL WHERE reconciliation_id = ?1", [id])?;
        tx.execute("DELETE FROM reconciliations WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(())
    }

    /// Reopens the most recent completed statement for an account: its lines
    /// go back to cleared so they can be fixed and reconciled again.
    pub fn undo_last_reconciliation(&mut self, account_id: AccountId) -> Result<()> {
        let tx = self.conn.savepoint()?;
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
