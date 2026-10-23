//! Tax-center persistence: Schedule C mapping, payees and 1099 totals,
//! estimated tax payments. The arithmetic lives in `tally_core::tax`.
//!
//! Only the business side of a book is taxed here: Schedule C is a business
//! form, so household categories are neither mapped nor reported.

use crate::{BadValue, Result, Store, StoreError, check_entry_not_locked, conversion_err, post_in, rule};
use chrono::NaiveDate;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tally_core::journal::EntryId;
use tally_core::tax::{self, ScheduleC, ScheduleCLine};
use tally_core::{AccountId, AccountKind, AccountSubtype, Cents, EntryLine, NewEntry, Scope};

pub type TaxPaymentId = i64;

/// Owner's Draw: estimated income tax for a sole proprietor is a personal
/// expense paid with business money, not a business deduction.
const OWNERS_DRAW: &str = "3100";

fn line_str(line: ScheduleCLine) -> String {
    serde_json::to_value(line).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

fn parse_line(s: &str) -> Option<ScheduleCLine> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
}

/// One income/expense account and where it lands on Schedule C.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaxMappingRow {
    pub account_id: AccountId,
    pub code: Option<String>,
    pub name: String,
    pub kind: AccountKind,
    /// Natural-sign activity for the year.
    pub amount: Cents,
    pub line: ScheduleCLine,
    /// False when `line` is only Tallybook's suggestion.
    pub confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScheduleCReport {
    pub year: i32,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub schedule: ScheduleC,
    pub rows: Vec<TaxMappingRow>,
    /// Accounts with activity whose line hasn't been confirmed.
    pub unconfirmed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Payee1099 {
    pub contact_id: i64,
    pub name: String,
    pub legal_name: Option<String>,
    pub is_1099: bool,
    pub w9_on_file: bool,
    pub tin_last4: Option<String>,
    /// Paid from bank accounts (check, ACH, transfer, cash withdrawals).
    pub paid_reportable: Cents,
    /// Paid by credit card; the card network reports these, not you.
    pub paid_by_card: Cents,
    pub over_threshold: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report1099 {
    pub year: i32,
    pub threshold: Cents,
    pub rows: Vec<Payee1099>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Jurisdiction {
    Federal,
    State,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaxPayment {
    pub id: TaxPaymentId,
    pub tax_year: i32,
    pub quarter: u8,
    pub jurisdiction: Jurisdiction,
    pub date: NaiveDate,
    pub amount: Cents,
    pub account_id: AccountId,
    pub entry_id: EntryId,
    pub voided: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewTaxPayment {
    pub tax_year: i32,
    pub quarter: u8,
    pub jurisdiction: Jurisdiction,
    pub date: NaiveDate,
    pub amount: Cents,
    /// Paid from (bank or card).
    pub account_id: AccountId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContactTax {
    pub is_1099: bool,
    pub w9_on_file: bool,
    pub legal_name: Option<String>,
    pub tin_last4: Option<String>,
}

/// January 1 through December 31 of `year`. The year can come straight from a
/// request, so one outside the calendar's range is an error, not a panic.
pub fn year_range(year: i32) -> Result<(NaiveDate, NaiveDate)> {
    match (NaiveDate::from_ymd_opt(year, 1, 1), NaiveDate::from_ymd_opt(year, 12, 31)) {
        (Some(first), Some(last)) => Ok((first, last)),
        _ => rule(format!("{year} isn't a year Tallybook can work with")),
    }
}

impl Store {
    // ------------------------------------------------------------ Schedule C

    /// Confirms which Schedule C line a business income or expense account
    /// feeds; `None` goes back to the suggested line.
    pub fn set_tax_line(&self, account_id: AccountId, line: Option<ScheduleCLine>) -> Result<()> {
        let account = self.account(account_id)?;
        if account.kind().is_balance_sheet() {
            return rule("only income and expense accounts go on Schedule C");
        }
        if account.scope == Scope::Household {
            return rule("household categories don't go on Schedule C");
        }
        match line {
            Some(l) => self.conn.execute(
                "INSERT INTO account_tax_lines(account_id, line) VALUES (?1, ?2)
                 ON CONFLICT(account_id) DO UPDATE SET line = excluded.line",
                params![account_id, line_str(l)],
            )?,
            None => self.conn.execute("DELETE FROM account_tax_lines WHERE account_id = ?1", [account_id])?,
        };
        Ok(())
    }

    fn confirmed_lines(&self) -> Result<HashMap<AccountId, ScheduleCLine>> {
        let mut stmt = self.conn.prepare("SELECT account_id, line FROM account_tax_lines")?;
        let rows = stmt
            .query_map([], |r| {
                let line: String = r.get(1)?;
                Ok((r.get::<_, AccountId>(0)?, parse_line(&line).ok_or_else(|| conversion_err(1, BadValue(line)))?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Schedule C for a calendar year from the books plus the line mapping.
    /// `home_office` is a figure the user supplies (simplified method or Form 8829).
    pub fn schedule_c(&self, year: i32, home_office: Cents) -> Result<ScheduleCReport> {
        let (from, to) = year_range(year)?;
        let balances = self.balances(Some(from), to)?;
        let confirmed = self.confirmed_lines()?;
        let mut rows = Vec::new();
        // Household spending isn't a business deduction.
        for a in self.accounts_in(Some(Scope::Business))? {
            if a.kind().is_balance_sheet() {
                continue;
            }
            let amount = a.kind().natural(balances.get(&a.id).copied().unwrap_or_default());
            let (line, is_confirmed) = match confirmed.get(&a.id) {
                Some(l) => (*l, true),
                None => (tax::suggest_line(&a.name, a.kind() == AccountKind::Income, a.subtype == AccountSubtype::CostOfGoodsSold), false),
            };
            if amount.is_zero() && a.archived {
                continue;
            }
            rows.push(TaxMappingRow { account_id: a.id, kind: a.kind(), code: a.code, name: a.name, amount, line, confirmed: is_confirmed });
        }
        let amounts: Vec<(ScheduleCLine, Cents)> = rows
            .iter()
            .map(|r| {
                // A contra-income account (refunds) has a debit balance, i.e. negative
                // natural income; line 2 wants it as a positive figure.
                let amount = if r.line == ScheduleCLine::L2Returns && r.kind == AccountKind::Income { -r.amount } else { r.amount };
                (r.line, amount)
            })
            .collect();
        let unconfirmed = rows.iter().filter(|r| !r.confirmed && !r.amount.is_zero()).count();
        Ok(ScheduleCReport { year, from, to, schedule: tax::schedule_c(&amounts, home_office), rows, unconfirmed })
    }

    // ------------------------------------------------------------ payees & 1099s

    /// Records who an entry was with. Amounts are untouched, but the payee
    /// decides 1099 totals, so entries in closed periods keep theirs.
    pub fn set_entry_payee(&self, entry_id: EntryId, contact_id: Option<i64>) -> Result<()> {
        check_entry_not_locked(&self.conn, entry_id)?;
        if let Some(c) = contact_id {
            self.contact(c)?;
        }
        if self.conn.execute("UPDATE journal_entries SET payee_id = ?1 WHERE id = ?2", params![contact_id, entry_id])? == 0 {
            return Err(StoreError::NotFound("entry", entry_id));
        }
        Ok(())
    }

    pub fn set_contact_tax(&self, id: i64, t: &ContactTax) -> Result<()> {
        let last4 = t.tin_last4.as_deref().map(str::trim).filter(|s| !s.is_empty());
        if let Some(l) = last4
            && (l.len() != 4 || !l.chars().all(|c| c.is_ascii_digit()))
        {
            return rule("enter only the last four digits of the tax ID");
        }
        let n = self.conn.execute(
            "UPDATE contacts SET is_1099 = ?1, w9_on_file = ?2, legal_name = ?3, tin_last4 = ?4 WHERE id = ?5",
            params![t.is_1099, t.w9_on_file, t.legal_name.as_deref().map(str::trim).filter(|s| !s.is_empty()), last4, id],
        )?;
        if n == 0 {
            return Err(StoreError::NotFound("contact", id));
        }
        Ok(())
    }

    /// Payments to each payee in `year`, split into what's reportable on a
    /// 1099-NEC (money out of bank accounts) and what isn't (credit card).
    pub fn report_1099(&self, year: i32) -> Result<Report1099> {
        let ty = tax::tax_year(year).ok_or_else(|| StoreError::Rule(format!("Tallybook doesn't have {year} tax tables yet")))?;
        let threshold = Cents(ty.nec_threshold * 100);
        let (from, to) = year_range(year)?;
        let mut stmt = self.conn.prepare(
            "SELECT c.id, c.name, c.legal_name, c.is_1099, c.w9_on_file, c.tin_last4,
                    IFNULL(SUM(CASE WHEN a.subtype = 'bank' THEN -l.amount END), 0),
                    IFNULL(SUM(CASE WHEN a.subtype = 'credit_card' THEN -l.amount END), 0)
             FROM contacts c
             JOIN journal_entries e ON e.payee_id = c.id
             JOIN journal_lines l ON l.entry_id = e.id
             JOIN accounts a ON a.id = l.account_id
             WHERE e.date BETWEEN ?1 AND ?2 AND e.voided_by_id IS NULL AND e.reverses_id IS NULL
               AND c.kind <> 'customer'
             GROUP BY c.id
             HAVING c.is_1099 = 1
                 OR SUM(CASE WHEN a.subtype IN ('bank', 'credit_card') THEN -l.amount ELSE 0 END) > 0
             ORDER BY c.name COLLATE NOCASE",
        )?;
        let rows = stmt
            .query_map(params![from, to], |r| {
                let is_1099: bool = r.get(3)?;
                let paid = Cents(r.get(6)?);
                Ok(Payee1099 {
                    contact_id: r.get(0)?,
                    name: r.get(1)?,
                    legal_name: r.get(2)?,
                    is_1099,
                    w9_on_file: r.get(4)?,
                    tin_last4: r.get(5)?,
                    paid_reportable: paid,
                    paid_by_card: Cents(r.get(7)?),
                    over_threshold: is_1099 && paid >= threshold,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(Report1099 { year, threshold, rows })
    }

    // ------------------------------------------------------------ estimated payments

    /// Records an estimated tax payment and posts it to Owner's Draw.
    pub fn record_tax_payment(&mut self, p: &NewTaxPayment) -> Result<TaxPaymentId> {
        if !(1..=4).contains(&p.quarter) {
            return rule("quarter must be 1–4");
        }
        if p.amount.0 <= 0 {
            return rule("enter the amount you paid");
        }
        let from = self.account(p.account_id)?;
        if !matches!(from.subtype, AccountSubtype::Bank | AccountSubtype::CreditCard | AccountSubtype::OtherCurrentAsset) {
            return rule(format!("{} can't be used to pay", from.name));
        }
        let draw = self
            .account_by_code(OWNERS_DRAW)?
            .ok_or_else(|| StoreError::Rule("the Owner's Draw account (3100) is missing from the chart".into()))?;
        let who = match p.jurisdiction {
            Jurisdiction::Federal => "Federal",
            Jurisdiction::State => "State",
        };
        let entry = NewEntry::new(p.date, format!("{who} estimated tax · Q{} {}", p.quarter, p.tax_year))
            .line(EntryLine::debit(draw.id, p.amount))
            .line(EntryLine::credit(p.account_id, p.amount));
        let tx = self.conn.transaction()?;
        let entry_id = post_in(&tx, &entry, "tax_payment", None)?;
        tx.execute(
            "INSERT INTO tax_payments(tax_year, quarter, jurisdiction, date, amount, account_id, entry_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                p.tax_year,
                p.quarter,
                if p.jurisdiction == Jurisdiction::Federal { "federal" } else { "state" },
                p.date,
                p.amount.0,
                p.account_id,
                entry_id
            ],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    }

    pub fn tax_payments(&self, year: i32) -> Result<Vec<TaxPayment>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, tax_year, quarter, jurisdiction, date, amount, account_id, entry_id, voided
             FROM tax_payments WHERE tax_year = ?1 ORDER BY date, id",
        )?;
        let rows = stmt
            .query_map([year], |r| {
                let j: String = r.get(3)?;
                Ok(TaxPayment {
                    id: r.get(0)?,
                    tax_year: r.get(1)?,
                    quarter: r.get(2)?,
                    jurisdiction: if j == "federal" { Jurisdiction::Federal } else { Jurisdiction::State },
                    date: r.get(4)?,
                    amount: Cents(r.get(5)?),
                    account_id: r.get(6)?,
                    entry_id: r.get(7)?,
                    voided: r.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn void_tax_payment(&mut self, id: TaxPaymentId, date: NaiveDate) -> Result<()> {
        let row: Option<(bool, EntryId)> = self
            .conn
            .query_row("SELECT voided, entry_id FROM tax_payments WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        let Some((voided, entry_id)) = row else { return Err(StoreError::NotFound("tax payment", id)) };
        if voided {
            return rule("this payment is already void");
        }
        self.void_entry(entry_id, date)?;
        self.conn.execute("UPDATE tax_payments SET voided = 1 WHERE id = ?1", [id])?;
        Ok(())
    }
}
