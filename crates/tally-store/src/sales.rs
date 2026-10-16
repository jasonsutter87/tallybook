//! Customers & vendors, invoices, bills, and payments.
//!
//! Accounting each document produces:
//! * **Invoice** (finalized): Dr A/R total · Cr income per line · Cr Sales Tax Payable
//! * **Payment received**: Dr bank/Undeposited Funds · Cr A/R
//! * **Bill**: Dr expense per line · Cr A/P
//! * **Bill payment**: Dr A/P · Cr bank/card
//!
//! Drafts have no entry and can be edited or deleted. Everything else is
//! voided, never edited, like the rest of the ledger.

use crate::{Result, Store, StoreError, conversion_err, BadValue, entry_in, post_in, rule, setting, set_setting};
use chrono::NaiveDate;
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tally_core::chart::codes;
use tally_core::invoice::{self, AgingBuckets, LineInput};
use tally_core::journal::EntryId;
use tally_core::{AccountId, AccountKind, AccountSubtype, Cents, EntryLine, NewEntry};

pub type ContactId = i64;
pub type InvoiceId = i64;
pub type BillId = i64;
pub type PaymentId = i64;

// ---------------------------------------------------------------- contacts

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContactKind {
    Customer,
    Vendor,
    Both,
}

impl ContactKind {
    fn as_str(self) -> &'static str {
        match self {
            ContactKind::Customer => "customer",
            ContactKind::Vendor => "vendor",
            ContactKind::Both => "both",
        }
    }

    fn is_customer(self) -> bool {
        self != ContactKind::Vendor
    }

    fn is_vendor(self) -> bool {
        self != ContactKind::Customer
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewContact {
    pub kind: ContactKind,
    pub name: String,
    pub company: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Contact {
    pub id: ContactId,
    pub kind: ContactKind,
    pub name: String,
    pub company: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub notes: Option<String>,
    pub archived: bool,
    /// Customers: unpaid invoices. Vendors: unpaid bills.
    pub open_balance: Cents,
}

const CONTACT_COLS: &str = "c.id, c.kind, c.name, c.company, c.email, c.phone, c.address, c.notes, c.archived";

/// Open balance (invoices + bills) per contact.
const CONTACT_BALANCE: &str = "
    IFNULL((SELECT SUM(i.total - IFNULL((SELECT SUM(pa.amount) FROM payment_applications pa
                JOIN payments p ON p.id = pa.payment_id WHERE pa.invoice_id = i.id AND p.voided = 0), 0))
            FROM invoices i WHERE i.customer_id = c.id AND i.status = 'open'), 0)
  + IFNULL((SELECT SUM(b.total - IFNULL((SELECT SUM(pa.amount) FROM payment_applications pa
                JOIN payments p ON p.id = pa.payment_id WHERE pa.bill_id = b.id AND p.voided = 0), 0))
            FROM bills b WHERE b.vendor_id = c.id AND b.status = 'open'), 0)";

fn contact_from_row(r: &Row) -> rusqlite::Result<Contact> {
    let kind: String = r.get(1)?;
    Ok(Contact {
        id: r.get(0)?,
        kind: match kind.as_str() {
            "customer" => ContactKind::Customer,
            "vendor" => ContactKind::Vendor,
            "both" => ContactKind::Both,
            _ => return Err(conversion_err(1, BadValue(kind))),
        },
        name: r.get(2)?,
        company: r.get(3)?,
        email: r.get(4)?,
        phone: r.get(5)?,
        address: r.get(6)?,
        notes: r.get(7)?,
        archived: r.get(8)?,
        open_balance: Cents(r.get(9)?),
    })
}

fn clean(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

// ---------------------------------------------------------------- documents

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvoiceLineInput {
    pub description: String,
    pub quantity_milli: i64,
    pub unit_price: Cents,
    pub account_id: AccountId,
    pub taxable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvoiceDraft {
    pub customer_id: ContactId,
    pub issue_date: NaiveDate,
    pub due_date: NaiveDate,
    pub memo: String,
    pub tax_rate_bp: u32,
    pub lines: Vec<InvoiceLineInput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DocStatus {
    Draft,
    Open,
    Partial,
    Paid,
    Overdue,
    Void,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InvoiceSummary {
    pub id: InvoiceId,
    pub number: String,
    pub customer_id: ContactId,
    pub customer_name: String,
    pub issue_date: NaiveDate,
    pub due_date: NaiveDate,
    pub total: Cents,
    pub paid: Cents,
    pub balance: Cents,
    pub status: DocStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InvoiceLine {
    pub description: String,
    pub quantity_milli: i64,
    pub unit_price: Cents,
    pub amount: Cents,
    pub account_id: AccountId,
    pub taxable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppliedPayment {
    pub payment_id: PaymentId,
    pub date: NaiveDate,
    pub amount: Cents,
    pub method: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InvoiceDetail {
    #[serde(flatten)]
    pub summary: InvoiceSummary,
    pub memo: String,
    pub tax_rate_bp: u32,
    pub subtotal: Cents,
    pub tax: Cents,
    pub entry_id: Option<EntryId>,
    pub lines: Vec<InvoiceLine>,
    pub payments: Vec<AppliedPayment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BillLineInput {
    pub description: String,
    pub amount: Cents,
    pub account_id: AccountId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewBill {
    pub vendor_id: ContactId,
    /// The vendor's own invoice number.
    pub number: String,
    pub issue_date: NaiveDate,
    pub due_date: NaiveDate,
    pub memo: String,
    pub lines: Vec<BillLineInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BillSummary {
    pub id: BillId,
    pub number: String,
    pub vendor_id: ContactId,
    pub vendor_name: String,
    pub issue_date: NaiveDate,
    pub due_date: NaiveDate,
    pub total: Cents,
    pub paid: Cents,
    pub balance: Cents,
    pub status: DocStatus,
    pub memo: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Application {
    /// An invoice id for payments received, a bill id for payments made.
    pub document_id: i64,
    pub amount: Cents,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewPayment {
    pub contact_id: ContactId,
    pub date: NaiveDate,
    /// Deposit-to account (received) or paid-from account (made).
    pub account_id: AccountId,
    pub method: String,
    pub reference: String,
    pub memo: String,
    pub applications: Vec<Application>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgingRow {
    pub contact_id: ContactId,
    pub name: String,
    pub buckets: AgingBuckets,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgingReport {
    pub as_of: NaiveDate,
    pub rows: Vec<AgingRow>,
    pub totals: AgingBuckets,
}

fn status_of(doc_status: &str, balance: Cents, paid: Cents, due: NaiveDate, today: NaiveDate) -> DocStatus {
    match doc_status {
        "draft" => DocStatus::Draft,
        "void" => DocStatus::Void,
        _ if balance.0 <= 0 => DocStatus::Paid,
        _ if due < today => DocStatus::Overdue,
        _ if paid.0 > 0 => DocStatus::Partial,
        _ => DocStatus::Open,
    }
}

const INVOICE_SUMMARY_SQL: &str = "
    SELECT i.id, i.number, i.customer_id, c.name, i.issue_date, i.due_date, i.total, i.status,
           IFNULL((SELECT SUM(pa.amount) FROM payment_applications pa JOIN payments p ON p.id = pa.payment_id
                   WHERE pa.invoice_id = i.id AND p.voided = 0), 0) AS paid
    FROM invoices i JOIN contacts c ON c.id = i.customer_id";

fn invoice_summary_from_row(r: &Row, today: NaiveDate) -> rusqlite::Result<InvoiceSummary> {
    let total = Cents(r.get(6)?);
    let status: String = r.get(7)?;
    let paid = Cents(r.get(8)?);
    let due: NaiveDate = r.get(5)?;
    let balance = if status == "open" { total - paid } else { Cents::ZERO };
    Ok(InvoiceSummary {
        id: r.get(0)?,
        number: r.get(1)?,
        customer_id: r.get(2)?,
        customer_name: r.get(3)?,
        issue_date: r.get(4)?,
        due_date: due,
        total,
        paid,
        balance,
        status: status_of(&status, if status == "draft" { total } else { balance }, paid, due, today),
    })
}

const BILL_SUMMARY_SQL: &str = "
    SELECT b.id, b.number, b.vendor_id, c.name, b.issue_date, b.due_date, b.total, b.status,
           IFNULL((SELECT SUM(pa.amount) FROM payment_applications pa JOIN payments p ON p.id = pa.payment_id
                   WHERE pa.bill_id = b.id AND p.voided = 0), 0) AS paid, b.memo
    FROM bills b JOIN contacts c ON c.id = b.vendor_id";

fn bill_summary_from_row(r: &Row, today: NaiveDate) -> rusqlite::Result<BillSummary> {
    let total = Cents(r.get(6)?);
    let status: String = r.get(7)?;
    let paid = Cents(r.get(8)?);
    let due: NaiveDate = r.get(5)?;
    let balance = if status == "open" { total - paid } else { Cents::ZERO };
    Ok(BillSummary {
        id: r.get(0)?,
        number: r.get(1)?,
        vendor_id: r.get(2)?,
        vendor_name: r.get(3)?,
        issue_date: r.get(4)?,
        due_date: due,
        total,
        paid,
        balance,
        status: status_of(&status, balance, paid, due, today),
        memo: r.get(9)?,
    })
}

impl Store {
    // ------------------------------------------------------------ settings (public, namespaced)

    /// Free-form preferences: `company.name`, `invoice.next_number`, ...
    pub fn pref(&self, key: &str) -> Result<Option<String>> {
        setting(&self.conn, &format!("pref.{key}"))
    }

    pub fn set_pref(&self, key: &str, value: Option<&str>) -> Result<()> {
        set_setting(&self.conn, &format!("pref.{key}"), value)
    }

    fn required_account(&self, code: &str, what: &str) -> Result<AccountId> {
        self.account_by_code(code)?
            .map(|a| a.id)
            .ok_or_else(|| StoreError::Rule(format!("the {what} account ({code}) is missing from the chart")))
    }

    // ------------------------------------------------------------ contacts

    pub fn create_contact(&self, c: &NewContact) -> Result<ContactId> {
        if c.name.trim().is_empty() {
            return rule("a name is required");
        }
        self.conn.execute(
            "INSERT INTO contacts(kind, name, company, email, phone, address, notes) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![c.kind.as_str(), c.name.trim(), clean(&c.company), clean(&c.email), clean(&c.phone), clean(&c.address), clean(&c.notes)],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_contact(&self, id: ContactId, c: &NewContact) -> Result<()> {
        if c.name.trim().is_empty() {
            return rule("a name is required");
        }
        let n = self.conn.execute(
            "UPDATE contacts SET kind = ?1, name = ?2, company = ?3, email = ?4, phone = ?5, address = ?6, notes = ?7 WHERE id = ?8",
            params![c.kind.as_str(), c.name.trim(), clean(&c.company), clean(&c.email), clean(&c.phone), clean(&c.address), clean(&c.notes), id],
        )?;
        if n == 0 {
            return Err(StoreError::NotFound("contact", id));
        }
        Ok(())
    }

    pub fn set_contact_archived(&self, id: ContactId, archived: bool) -> Result<()> {
        self.conn.execute("UPDATE contacts SET archived = ?1 WHERE id = ?2", params![archived, id])?;
        Ok(())
    }

    pub fn contacts(&self) -> Result<Vec<Contact>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {CONTACT_COLS}, {CONTACT_BALANCE} FROM contacts c ORDER BY c.archived, c.name COLLATE NOCASE"
        ))?;
        Ok(stmt.query_map([], contact_from_row)?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn contact(&self, id: ContactId) -> Result<Contact> {
        self.conn
            .query_row(&format!("SELECT {CONTACT_COLS}, {CONTACT_BALANCE} FROM contacts c WHERE c.id = ?1"), [id], contact_from_row)
            .optional()?
            .ok_or(StoreError::NotFound("contact", id))
    }

    // ------------------------------------------------------------ invoices

    fn next_invoice_number(&self) -> Result<String> {
        let mut n: u64 = self.pref("invoice.next_number")?.and_then(|s| s.parse().ok()).unwrap_or(1001);
        loop {
            let candidate = n.to_string();
            let taken: bool = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM invoices WHERE number = ?1)", [&candidate], |r| r.get(0))?;
            if !taken {
                self.set_pref("invoice.next_number", Some(&(n + 1).to_string()))?;
                return Ok(candidate);
            }
            n += 1;
        }
    }

    fn check_invoice_draft(&self, d: &InvoiceDraft) -> Result<invoice::Totals> {
        let customer = self.contact(d.customer_id)?;
        if !customer.kind.is_customer() {
            return rule(format!("{} isn't set up as a customer", customer.name));
        }
        if d.due_date < d.issue_date {
            return rule("due date is before the invoice date");
        }
        if d.lines.is_empty() {
            return rule("add at least one line");
        }
        if d.tax_rate_bp > 10_000 {
            return rule("tax rate can't exceed 100%");
        }
        for (i, l) in d.lines.iter().enumerate() {
            let a = self.account(l.account_id)?;
            if a.kind() != AccountKind::Income {
                return rule(format!("line {}: {} isn't an income account", i + 1, a.name));
            }
            if l.unit_price.0 < 0 {
                return rule(format!("line {}: price can't be negative", i + 1));
            }
        }
        let inputs: Vec<LineInput> =
            d.lines.iter().map(|l| LineInput { quantity_milli: l.quantity_milli, unit_price: l.unit_price, taxable: l.taxable }).collect();
        let totals = invoice::totals(&inputs, d.tax_rate_bp).map_err(|e| StoreError::Rule(e.to_string()))?;
        if totals.total.0 <= 0 {
            return rule("invoice total must be more than zero");
        }
        Ok(totals)
    }

    fn write_invoice_lines(tx: &rusqlite::Transaction, id: InvoiceId, d: &InvoiceDraft) -> Result<()> {
        tx.execute("DELETE FROM invoice_lines WHERE invoice_id = ?1", [id])?;
        for (pos, l) in d.lines.iter().enumerate() {
            let amount = invoice::line_amount(l.quantity_milli, l.unit_price).ok_or_else(|| StoreError::Rule("line amount overflow".into()))?;
            tx.execute(
                "INSERT INTO invoice_lines(invoice_id, position, description, quantity_milli, unit_price, amount, account_id, taxable)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![id, pos as i64, l.description.trim(), l.quantity_milli, l.unit_price.0, amount.0, l.account_id, l.taxable],
            )?;
        }
        Ok(())
    }

    /// Saves a draft invoice and assigns it the next number.
    pub fn create_invoice(&mut self, d: &InvoiceDraft) -> Result<InvoiceId> {
        let totals = self.check_invoice_draft(d)?;
        let number = self.next_invoice_number()?;
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO invoices(number, customer_id, issue_date, due_date, memo, tax_rate_bp, subtotal, tax, total)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![number, d.customer_id, d.issue_date, d.due_date, d.memo.trim(), d.tax_rate_bp, totals.subtotal.0, totals.tax.0, totals.total.0],
        )?;
        let id = tx.last_insert_rowid();
        Self::write_invoice_lines(&tx, id, d)?;
        tx.commit()?;
        Ok(id)
    }

    pub fn update_invoice(&mut self, id: InvoiceId, d: &InvoiceDraft) -> Result<()> {
        self.require_invoice_status(id, "draft", "only drafts can be edited; void and re-issue instead")?;
        let totals = self.check_invoice_draft(d)?;
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE invoices SET customer_id = ?1, issue_date = ?2, due_date = ?3, memo = ?4, tax_rate_bp = ?5,
                    subtotal = ?6, tax = ?7, total = ?8 WHERE id = ?9",
            params![d.customer_id, d.issue_date, d.due_date, d.memo.trim(), d.tax_rate_bp, totals.subtotal.0, totals.tax.0, totals.total.0, id],
        )?;
        Self::write_invoice_lines(&tx, id, d)?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_draft_invoice(&mut self, id: InvoiceId) -> Result<()> {
        self.require_invoice_status(id, "draft", "only drafts can be deleted; void it instead")?;
        self.conn.execute("DELETE FROM invoices WHERE id = ?1", [id])?;
        Ok(())
    }

    fn require_invoice_status(&self, id: InvoiceId, want: &str, msg: &str) -> Result<()> {
        let status: Option<String> = self.conn.query_row("SELECT status FROM invoices WHERE id = ?1", [id], |r| r.get(0)).optional()?;
        match status {
            None => Err(StoreError::NotFound("invoice", id)),
            Some(s) if s == want => Ok(()),
            Some(_) => rule(msg),
        }
    }

    /// Posts a draft to the books: it becomes an open receivable.
    pub fn finalize_invoice(&mut self, id: InvoiceId) -> Result<EntryId> {
        self.require_invoice_status(id, "draft", "this invoice was already issued")?;
        let detail = self.invoice(id, NaiveDate::MIN)?;
        let ar = self.required_account(codes::ACCOUNTS_RECEIVABLE, "Accounts Receivable")?;

        let mut by_account: BTreeMap<AccountId, Cents> = BTreeMap::new();
        for l in &detail.lines {
            *by_account.entry(l.account_id).or_default() += l.amount;
        }
        let mut entry = NewEntry::new(detail.summary.issue_date, format!("Invoice {} · {}", detail.summary.number, detail.summary.customer_name))
            .line(EntryLine::debit(ar, detail.summary.total));
        for (account, amount) in by_account {
            if !amount.is_zero() {
                entry = entry.line(EntryLine::credit(account, amount));
            }
        }
        if !detail.tax.is_zero() {
            let tax_acct = self.required_account(codes::SALES_TAX_PAYABLE, "Sales Tax Payable")?;
            entry = entry.line(EntryLine::credit(tax_acct, detail.tax));
        }

        let tx = self.conn.transaction()?;
        let entry_id = post_in(&tx, &entry, "invoice", None)?;
        tx.execute("UPDATE invoices SET status = 'open', entry_id = ?1 WHERE id = ?2", params![entry_id, id])?;
        tx.commit()?;
        Ok(entry_id)
    }

    pub fn void_invoice(&mut self, id: InvoiceId, date: NaiveDate) -> Result<()> {
        self.require_invoice_status(id, "open", "only issued invoices can be voided (delete drafts instead)")?;
        let has_payments: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM payment_applications pa JOIN payments p ON p.id = pa.payment_id
                           WHERE pa.invoice_id = ?1 AND p.voided = 0)",
            [id],
            |r| r.get(0),
        )?;
        if has_payments {
            return rule("this invoice has payments; void those first");
        }
        let entry_id: EntryId = self.conn.query_row("SELECT entry_id FROM invoices WHERE id = ?1", [id], |r| r.get(0))?;
        self.void_entry(entry_id, date)?;
        self.conn.execute("UPDATE invoices SET status = 'void' WHERE id = ?1", [id])?;
        Ok(())
    }

    /// `today` drives the overdue status.
    pub fn invoices(&self, today: NaiveDate) -> Result<Vec<InvoiceSummary>> {
        let mut stmt = self.conn.prepare(&format!("{INVOICE_SUMMARY_SQL} ORDER BY i.issue_date DESC, i.id DESC"))?;
        Ok(stmt.query_map([], |r| invoice_summary_from_row(r, today))?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn invoice(&self, id: InvoiceId, today: NaiveDate) -> Result<InvoiceDetail> {
        let summary = self
            .conn
            .query_row(&format!("{INVOICE_SUMMARY_SQL} WHERE i.id = ?1"), [id], |r| invoice_summary_from_row(r, today))
            .optional()?
            .ok_or(StoreError::NotFound("invoice", id))?;
        let (memo, tax_rate_bp, subtotal, tax, entry_id): (String, u32, i64, i64, Option<EntryId>) = self.conn.query_row(
            "SELECT memo, tax_rate_bp, subtotal, tax, entry_id FROM invoices WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )?;
        let mut stmt = self.conn.prepare(
            "SELECT description, quantity_milli, unit_price, amount, account_id, taxable FROM invoice_lines WHERE invoice_id = ?1 ORDER BY position",
        )?;
        let lines = stmt
            .query_map([id], |r| {
                Ok(InvoiceLine {
                    description: r.get(0)?,
                    quantity_milli: r.get(1)?,
                    unit_price: Cents(r.get(2)?),
                    amount: Cents(r.get(3)?),
                    account_id: r.get(4)?,
                    taxable: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(InvoiceDetail {
            summary,
            memo,
            tax_rate_bp,
            subtotal: Cents(subtotal),
            tax: Cents(tax),
            entry_id,
            lines,
            payments: self.applied_payments("invoice_id", id)?,
        })
    }

    fn applied_payments(&self, column: &str, id: i64) -> Result<Vec<AppliedPayment>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT p.id, p.date, pa.amount, p.method FROM payment_applications pa JOIN payments p ON p.id = pa.payment_id
             WHERE pa.{column} = ?1 AND p.voided = 0 ORDER BY p.date, p.id"
        ))?;
        Ok(stmt
            .query_map([id], |r| Ok(AppliedPayment { payment_id: r.get(0)?, date: r.get(1)?, amount: Cents(r.get(2)?), method: r.get(3)? }))?
            .collect::<rusqlite::Result<_>>()?)
    }

    // ------------------------------------------------------------ bills

    pub fn create_bill(&mut self, b: &NewBill) -> Result<BillId> {
        let vendor = self.contact(b.vendor_id)?;
        if !vendor.kind.is_vendor() {
            return rule(format!("{} isn't set up as a vendor", vendor.name));
        }
        if b.due_date < b.issue_date {
            return rule("due date is before the bill date");
        }
        if b.lines.is_empty() {
            return rule("add at least one line");
        }
        let mut total = Cents::ZERO;
        for (i, l) in b.lines.iter().enumerate() {
            if l.amount.0 <= 0 {
                return rule(format!("line {}: amount must be positive", i + 1));
            }
            let a = self.account(l.account_id)?;
            if matches!(a.subtype, AccountSubtype::AccountsPayable | AccountSubtype::AccountsReceivable) {
                return rule(format!("line {}: choose an expense or asset account", i + 1));
            }
            total = total.checked_add(l.amount).ok_or_else(|| StoreError::Rule("bill total overflow".into()))?;
        }
        let ap = self.required_account(codes::ACCOUNTS_PAYABLE, "Accounts Payable")?;
        let label = if b.number.trim().is_empty() { vendor.name.clone() } else { format!("{} #{}", vendor.name, b.number.trim()) };
        let mut entry = NewEntry::new(b.issue_date, format!("Bill · {label}"));
        for l in &b.lines {
            entry = entry.line(EntryLine::debit(l.account_id, l.amount).with_memo(l.description.trim()));
        }
        entry = entry.line(EntryLine::credit(ap, total));

        let tx = self.conn.transaction()?;
        let entry_id = post_in(&tx, &entry, "bill", None)?;
        tx.execute(
            "INSERT INTO bills(vendor_id, number, issue_date, due_date, memo, total, entry_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![b.vendor_id, b.number.trim(), b.issue_date, b.due_date, b.memo.trim(), total.0, entry_id],
        )?;
        let id = tx.last_insert_rowid();
        for (pos, l) in b.lines.iter().enumerate() {
            tx.execute(
                "INSERT INTO bill_lines(bill_id, position, description, amount, account_id) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, pos as i64, l.description.trim(), l.amount.0, l.account_id],
            )?;
        }
        tx.commit()?;
        Ok(id)
    }

    pub fn void_bill(&mut self, id: BillId, date: NaiveDate) -> Result<()> {
        let row: Option<(String, EntryId)> =
            self.conn.query_row("SELECT status, entry_id FROM bills WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        let Some((status, entry_id)) = row else { return Err(StoreError::NotFound("bill", id)) };
        if status != "open" {
            return rule("this bill is already void");
        }
        if !self.applied_payments("bill_id", id)?.is_empty() {
            return rule("this bill has payments; void those first");
        }
        self.void_entry(entry_id, date)?;
        self.conn.execute("UPDATE bills SET status = 'void' WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn bills(&self, today: NaiveDate) -> Result<Vec<BillSummary>> {
        let mut stmt = self.conn.prepare(&format!("{BILL_SUMMARY_SQL} ORDER BY b.due_date DESC, b.id DESC"))?;
        Ok(stmt.query_map([], |r| bill_summary_from_row(r, today))?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn bill(&self, id: BillId, today: NaiveDate) -> Result<(BillSummary, Vec<BillLineInput>, Vec<AppliedPayment>)> {
        let summary = self
            .conn
            .query_row(&format!("{BILL_SUMMARY_SQL} WHERE b.id = ?1"), [id], |r| bill_summary_from_row(r, today))
            .optional()?
            .ok_or(StoreError::NotFound("bill", id))?;
        let mut stmt = self.conn.prepare("SELECT description, amount, account_id FROM bill_lines WHERE bill_id = ?1 ORDER BY position")?;
        let lines = stmt
            .query_map([id], |r| Ok(BillLineInput { description: r.get(0)?, amount: Cents(r.get(1)?), account_id: r.get(2)? }))?
            .collect::<rusqlite::Result<_>>()?;
        Ok((summary, lines, self.applied_payments("bill_id", id)?))
    }

    // ------------------------------------------------------------ payments

    /// Records money in from a customer against one or more open invoices.
    pub fn receive_payment(&mut self, p: &NewPayment) -> Result<PaymentId> {
        self.record_payment(p, true)
    }

    /// Records money out to a vendor against one or more open bills.
    pub fn pay_bills(&mut self, p: &NewPayment) -> Result<PaymentId> {
        self.record_payment(p, false)
    }

    fn record_payment(&mut self, p: &NewPayment, received: bool) -> Result<PaymentId> {
        let contact = self.contact(p.contact_id)?;
        if p.applications.is_empty() {
            return rule(if received { "choose at least one invoice" } else { "choose at least one bill" });
        }
        let account = self.account(p.account_id)?;
        let ok_account = if received {
            matches!(account.subtype, AccountSubtype::Bank | AccountSubtype::OtherCurrentAsset)
        } else {
            matches!(account.subtype, AccountSubtype::Bank | AccountSubtype::OtherCurrentAsset | AccountSubtype::CreditCard)
        };
        if !ok_account {
            return rule(format!("{} can't be used for payments", account.name));
        }
        let mut total = Cents::ZERO;
        for a in &p.applications {
            if a.amount.0 <= 0 {
                return rule("payment amounts must be positive");
            }
            let open = if received {
                let inv = self.invoice(a.document_id, p.date)?;
                if inv.summary.customer_id != p.contact_id {
                    return rule(format!("invoice {} belongs to a different customer", inv.summary.number));
                }
                if inv.summary.status == DocStatus::Draft || inv.summary.status == DocStatus::Void {
                    return rule(format!("invoice {} isn't open", inv.summary.number));
                }
                inv.summary.balance
            } else {
                let (bill, _, _) = self.bill(a.document_id, p.date)?;
                if bill.vendor_id != p.contact_id {
                    return rule("that bill belongs to a different vendor");
                }
                bill.balance
            };
            if a.amount > open {
                return rule(format!("{} is more than the {} still owed", a.amount, open));
            }
            total = total.checked_add(a.amount).ok_or_else(|| StoreError::Rule("payment total overflow".into()))?;
        }

        let (control, memo) = if received {
            (self.required_account(codes::ACCOUNTS_RECEIVABLE, "Accounts Receivable")?, format!("Payment from {}", contact.name))
        } else {
            (self.required_account(codes::ACCOUNTS_PAYABLE, "Accounts Payable")?, format!("Payment to {}", contact.name))
        };
        let memo = if p.reference.trim().is_empty() { memo } else { format!("{memo} · {}", p.reference.trim()) };
        let entry = if received {
            NewEntry::new(p.date, memo).line(EntryLine::debit(p.account_id, total)).line(EntryLine::credit(control, total))
        } else {
            NewEntry::new(p.date, memo).line(EntryLine::debit(control, total)).line(EntryLine::credit(p.account_id, total))
        };

        let tx = self.conn.transaction()?;
        let entry_id = post_in(&tx, &entry, "payment", None)?;
        tx.execute(
            "INSERT INTO payments(kind, contact_id, date, amount, account_id, method, reference, memo, entry_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                if received { "received" } else { "made" },
                p.contact_id,
                p.date,
                total.0,
                p.account_id,
                p.method.trim(),
                p.reference.trim(),
                p.memo.trim(),
                entry_id
            ],
        )?;
        let id = tx.last_insert_rowid();
        let column = if received { "invoice_id" } else { "bill_id" };
        for a in &p.applications {
            tx.execute(
                &format!("INSERT INTO payment_applications(payment_id, {column}, amount) VALUES (?1, ?2, ?3)"),
                params![id, a.document_id, a.amount.0],
            )?;
        }
        tx.commit()?;
        Ok(id)
    }

    /// Voids a payment; its invoices/bills become unpaid again.
    pub fn void_payment(&mut self, id: PaymentId, date: NaiveDate) -> Result<()> {
        let row: Option<(bool, EntryId)> =
            self.conn.query_row("SELECT voided, entry_id FROM payments WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        let Some((voided, entry_id)) = row else { return Err(StoreError::NotFound("payment", id)) };
        if voided {
            return rule("this payment is already void");
        }
        if entry_in(&self.conn, entry_id)?.voided_by_id.is_none() {
            self.void_entry(entry_id, date)?;
        }
        self.conn.execute("UPDATE payments SET voided = 1 WHERE id = ?1", [id])?;
        Ok(())
    }

    // ------------------------------------------------------------ aging

    pub fn ar_aging(&self, as_of: NaiveDate) -> Result<AgingReport> {
        let docs: Vec<(ContactId, String, NaiveDate, Cents)> = self
            .invoices(as_of)?
            .into_iter()
            .filter(|i| i.balance.0 > 0 && i.issue_date <= as_of)
            .map(|i| (i.customer_id, i.customer_name, i.due_date, i.balance))
            .collect();
        Ok(aging(as_of, docs))
    }

    pub fn ap_aging(&self, as_of: NaiveDate) -> Result<AgingReport> {
        let docs: Vec<(ContactId, String, NaiveDate, Cents)> = self
            .bills(as_of)?
            .into_iter()
            .filter(|b| b.balance.0 > 0 && b.issue_date <= as_of)
            .map(|b| (b.vendor_id, b.vendor_name, b.due_date, b.balance))
            .collect();
        Ok(aging(as_of, docs))
    }
}

fn aging(as_of: NaiveDate, docs: Vec<(ContactId, String, NaiveDate, Cents)>) -> AgingReport {
    // Keyed by lowercase name for case-insensitive ordering; the id keeps namesakes apart.
    let mut by_contact: BTreeMap<(String, ContactId), AgingRow> = BTreeMap::new();
    let mut totals = AgingBuckets::default();
    for (contact_id, name, due, open) in docs {
        by_contact
            .entry((name.to_lowercase(), contact_id))
            .or_insert_with(|| AgingRow { contact_id, name, buckets: AgingBuckets::default() })
            .buckets
            .add(due, as_of, open);
        totals.add(due, as_of, open);
    }
    AgingReport { as_of, rows: by_contact.into_values().collect(), totals }
}
