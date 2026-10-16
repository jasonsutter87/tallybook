//! Tallybook's application layer.
//!
//! The UI talks to exactly one function, [`App::call`]: a method name such as
//! `"invoices.create"` plus JSON params, returning JSON. The desktop shell
//! (Tauri) and the browser dev server both just forward to it, so the two can
//! never drift apart.
//!
//! Money crosses the boundary as integer cents; dates as `YYYY-MM-DD`.

mod dashboard;
mod demo;
pub mod secrets;
mod sync;

use chrono::{Local, NaiveDate};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use tally_core::rules::RuleId;
use tally_core::{AccountId, AccountSubtype, Cents, EntryLine, NewEntry};
use tally_feed::{csv_import, ofx};
use tally_store::*;
// The glob above brings in the store's one-arg `Result`; keep std's.
use std::result::Result;

pub use secrets::{Keychain, MemorySecrets, Secrets};

/// Error shape the UI receives. `code` is machine-readable; `message` is
/// written for humans and safe to show as-is.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ApiError {
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn new(code: &'static str, message: impl Into<String>) -> ApiError {
        ApiError { code, message: message.into() }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> ApiError {
        match e {
            StoreError::Rule(m) => ApiError::new("rule", m),
            StoreError::Entry(e) => ApiError::new("rule", e.to_string()),
            StoreError::NotFound(..) => ApiError::new("not_found", e.to_string()),
            StoreError::Sqlite(e) => ApiError::new("internal", format!("database error: {e}")),
        }
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

fn params<T: DeserializeOwned>(v: Value) -> ApiResult<T> {
    // Methods without params may be called with `null`.
    let v = if v.is_null() { json!({}) } else { v };
    serde_json::from_value(v).map_err(|e| ApiError::new("bad_params", e.to_string()))
}

fn ok<T: Serialize>(v: T) -> ApiResult<Value> {
    serde_json::to_value(v).map_err(|e| ApiError::new("internal", e.to_string()))
}

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

struct OpenBook {
    store: Store,
    path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RecentBook {
    path: PathBuf,
    name: String,
    last_opened: String,
}

pub struct App {
    book: Mutex<Option<OpenBook>>,
    secrets: Arc<dyn Secrets>,
    /// Where `recent.json` lives (the OS app-config dir).
    config_dir: PathBuf,
}

// ---------------------------------------------------------------- param types

#[derive(Deserialize)]
struct Id {
    id: i64,
}

#[derive(Deserialize)]
struct IdDate {
    id: i64,
    date: Option<NaiveDate>,
}

#[derive(Deserialize)]
struct MaybeAccount {
    account_id: Option<AccountId>,
}

#[derive(Deserialize)]
struct Range {
    from: NaiveDate,
    to: NaiveDate,
}

#[derive(Deserialize)]
struct AsOf {
    as_of: Option<NaiveDate>,
}

#[derive(Deserialize)]
struct CreateBook {
    path: PathBuf,
    company_name: String,
}

#[derive(Deserialize)]
struct OpenPath {
    path: PathBuf,
}

#[derive(Deserialize)]
struct CreateAccount {
    code: Option<String>,
    name: String,
    subtype: AccountSubtype,
    parent_id: Option<AccountId>,
    opening_balance: Option<Cents>,
    opening_date: Option<NaiveDate>,
}

#[derive(Deserialize)]
struct Rename {
    id: AccountId,
    name: String,
}

#[derive(Deserialize)]
struct Archive {
    id: i64,
    archived: bool,
}

#[derive(Deserialize)]
struct OpeningBalance {
    id: AccountId,
    date: NaiveDate,
    amount: Cents,
}

#[derive(Deserialize)]
struct PostEntry {
    date: NaiveDate,
    memo: String,
    lines: Vec<EntryLine>,
}

#[derive(Deserialize)]
struct Accept {
    id: BankTxnId,
    account_id: AccountId,
    memo: Option<String>,
    /// Also create a rule so future transactions like this auto-categorize.
    #[serde(default)]
    remember: bool,
}

#[derive(Deserialize)]
struct AcceptSplit {
    id: BankTxnId,
    splits: Vec<Split>,
    memo: Option<String>,
}

#[derive(Deserialize)]
struct MatchLine {
    id: BankTxnId,
    line_id: LineId,
}

#[derive(Deserialize)]
struct Ids {
    ids: Vec<i64>,
}

#[derive(Deserialize)]
struct Content {
    content: String,
}

#[derive(Deserialize)]
struct CsvImport {
    content: String,
    mapping: csv_import::CsvMapping,
    account_id: AccountId,
}

#[derive(Deserialize)]
struct OfxImport {
    content: String,
    index: usize,
    account_id: AccountId,
}

#[derive(Deserialize)]
struct UpdateRule {
    id: RuleId,
    rule: NewRule,
}

#[derive(Deserialize)]
struct StartRec {
    account_id: AccountId,
    statement_date: NaiveDate,
    statement_balance: Cents,
}

#[derive(Deserialize)]
struct Toggle {
    id: ReconciliationId,
    line_id: LineId,
    checked: bool,
}

#[derive(Deserialize)]
struct ToggleMany {
    id: ReconciliationId,
    line_ids: Vec<LineId>,
    checked: bool,
}

#[derive(Deserialize)]
struct AccountOnly {
    account_id: AccountId,
}

#[derive(Deserialize)]
struct UpdateContact {
    id: ContactId,
    contact: NewContact,
}

#[derive(Deserialize)]
struct PaymentsQuery {
    kind: Option<PaymentKind>,
    contact_id: Option<ContactId>,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
}

#[derive(Deserialize)]
struct UpdateInvoice {
    id: InvoiceId,
    draft: InvoiceDraft,
}

#[derive(Deserialize)]
struct SetSettings {
    prefs: Option<std::collections::BTreeMap<String, Option<String>>>,
    fiscal_year_start_month: Option<u32>,
    /// `Some(None)` clears the lock date.
    #[serde(default, deserialize_with = "double_option")]
    lock_date: Option<Option<NaiveDate>>,
}

fn double_option<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<NaiveDate>>, D::Error> {
    Option::<NaiveDate>::deserialize(d).map(Some)
}

// ---------------------------------------------------------------- views

#[derive(Serialize)]
struct AccountView {
    #[serde(flatten)]
    account: tally_core::Account,
    kind: tally_core::AccountKind,
    balance: Cents,
    link: Option<BankLink>,
    reconcilable: bool,
}

#[derive(Serialize)]
struct ReviewedView {
    #[serde(flatten)]
    txn: BankTxn,
    categories: Vec<AccountId>,
}

#[derive(Serialize)]
struct BookStatus {
    open: bool,
    path: Option<PathBuf>,
    company_name: Option<String>,
    today: NaiveDate,
}

#[derive(Serialize)]
struct SubtypeInfo {
    value: AccountSubtype,
    kind: tally_core::AccountKind,
}

#[derive(Serialize)]
struct SettingsView {
    prefs: std::collections::BTreeMap<String, String>,
    fiscal_year_start_month: u32,
    lock_date: Option<NaiveDate>,
}

#[derive(Serialize)]
struct OfxPreview {
    account: tally_core::bank::FeedAccount,
    count: usize,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
}

// ---------------------------------------------------------------- app

impl App {
    pub fn new(secrets: Arc<dyn Secrets>, config_dir: impl Into<PathBuf>) -> App {
        App { book: Mutex::new(None), secrets, config_dir: config_dir.into() }
    }

    fn lock(&self) -> MutexGuard<'_, Option<OpenBook>> {
        // A panic while holding the lock can't corrupt SQLite (transactions
        // roll back), so recover rather than wedge the app.
        self.book.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Runs `f` against the open book, returning its typed result.
    fn run<T>(&self, f: impl FnOnce(&mut Store) -> Result<T, StoreError>) -> ApiResult<T> {
        let mut guard = self.lock();
        let book = guard.as_mut().ok_or_else(|| ApiError::new("no_book", "no book is open"))?;
        Ok(f(&mut book.store)?)
    }

    /// Runs `f` against the open book, returning JSON.
    fn with<T: Serialize>(&self, f: impl FnOnce(&mut Store) -> Result<T, StoreError>) -> ApiResult<Value> {
        ok(self.run(f)?)
    }

    fn with_api<T: Serialize>(&self, f: impl FnOnce(&mut Store) -> ApiResult<T>) -> ApiResult<Value> {
        let mut guard = self.lock();
        let book = guard.as_mut().ok_or_else(|| ApiError::new("no_book", "no book is open"))?;
        ok(f(&mut book.store)?)
    }

    fn call_status(&self) -> ApiResult<Value> {
        let guard = self.lock();
        let book = guard.as_ref().ok_or_else(|| ApiError::new("no_book", "no book is open"))?;
        ok(json!({ "path": book.path, "company_name": book.store.pref("company.name")? }))
    }

    pub fn is_open(&self) -> bool {
        self.lock().is_some()
    }

    /// The single entry point.
    pub async fn call(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            // ---------------------------------------------------- book
            "book.status" => {
                let guard = self.lock();
                let (path, name) = match guard.as_ref() {
                    Some(b) => (Some(b.path.clone()), b.store.pref("company.name")?),
                    None => (None, None),
                };
                ok(BookStatus { open: guard.is_some(), path, company_name: name, today: today() })
            }
            "book.create" => {
                let p: CreateBook = params(p)?;
                self.create_book(&p.path, &p.company_name)
            }
            "book.create_demo" => {
                let p: OpenPath = params(p)?;
                self.create_book(&p.path, "Northwind Studio")?;
                self.with(|s| demo::seed(s, today()))?;
                self.call_status()
            }
            "book.open" => {
                let p: OpenPath = params(p)?;
                self.open_book(&p.path)
            }
            "book.close" => {
                *self.lock() = None;
                ok(())
            }
            "book.recent" => ok(self.recent()),
            "book.forget" => {
                let p: OpenPath = params(p)?;
                let mut recent = self.recent();
                recent.retain(|r| r.path != p.path);
                self.save_recent(&recent);
                ok(())
            }
            "book.default_dir" => ok(default_dir()),
            "book.backup" => self.backup(),

            // ---------------------------------------------------- settings
            "settings.get" => self.with(|s| {
                Ok(SettingsView { prefs: s.prefs()?, fiscal_year_start_month: s.fiscal_year_start_month()?, lock_date: s.lock_date()? })
            }),
            "settings.set" => {
                let p: SetSettings = params(p)?;
                self.with(|s| {
                    for (k, v) in p.prefs.unwrap_or_default() {
                        s.set_pref(&k, v.as_deref().map(str::trim).filter(|v| !v.is_empty()))?;
                    }
                    if let Some(m) = p.fiscal_year_start_month {
                        s.set_fiscal_year_start_month(m)?;
                    }
                    if let Some(lock) = p.lock_date {
                        s.set_lock_date(lock)?;
                    }
                    Ok(())
                })
            }

            // ---------------------------------------------------- dashboard & reports
            "dashboard" => self.with(|s| dashboard::build(s, today())),
            "reports.pnl" => {
                let p: Range = params(p)?;
                self.with(|s| s.profit_and_loss(p.from, p.to))
            }
            "reports.balance_sheet" => {
                let p: AsOf = params(p)?;
                self.with(|s| s.balance_sheet(p.as_of.unwrap_or_else(today)))
            }
            "reports.trial_balance" => {
                let p: AsOf = params(p)?;
                self.with(|s| s.trial_balance(p.as_of.unwrap_or_else(today)))
            }
            "reports.ar_aging" => {
                let p: AsOf = params(p)?;
                self.with(|s| s.ar_aging(p.as_of.unwrap_or_else(today)))
            }
            "reports.ap_aging" => {
                let p: AsOf = params(p)?;
                self.with(|s| s.ap_aging(p.as_of.unwrap_or_else(today)))
            }

            // ---------------------------------------------------- accounts & journal
            "accounts.list" => self.with(|s| {
                let balances = s.balances(None, today())?;
                s.accounts()?
                    .into_iter()
                    .map(|a| {
                        Ok(AccountView {
                            kind: a.kind(),
                            balance: a.kind().natural(balances.get(&a.id).copied().unwrap_or_default()),
                            link: s.link_for_account(a.id)?,
                            reconcilable: a.subtype.is_reconcilable(),
                            account: a,
                        })
                    })
                    .collect::<Result<Vec<_>, StoreError>>()
            }),
            "accounts.subtypes" => ok(AccountSubtype::ALL.iter().map(|t| SubtypeInfo { value: *t, kind: t.kind() }).collect::<Vec<_>>()),
            "accounts.create" => {
                let p: CreateAccount = params(p)?;
                self.with(|s| {
                    let id = s.create_account(&NewAccount { code: p.code, name: p.name, subtype: p.subtype, parent_id: p.parent_id })?;
                    if let Some(amount) = p.opening_balance.filter(|a| !a.is_zero()) {
                        s.post_opening_balance(id, p.opening_date.unwrap_or_else(today), amount)?;
                    }
                    Ok(id)
                })
            }
            "accounts.rename" => {
                let p: Rename = params(p)?;
                self.with(|s| s.rename_account(p.id, &p.name))
            }
            "accounts.archive" => {
                let p: Archive = params(p)?;
                self.with(|s| s.set_archived(p.id, p.archived))
            }
            "accounts.register" => {
                let p: Id = params(p)?;
                self.with(|s| s.register(p.id))
            }
            "accounts.opening_balance" => {
                let p: OpeningBalance = params(p)?;
                self.with(|s| s.post_opening_balance(p.id, p.date, p.amount))
            }
            "entries.get" => {
                let p: Id = params(p)?;
                self.with(|s| s.entry(p.id))
            }
            "entries.list" => {
                let p: Range = params(p)?;
                self.with(|s| s.entries(p.from, p.to))
            }
            "entries.post" => {
                let p: PostEntry = params(p)?;
                self.with(|s| s.post_entry(&NewEntry { date: p.date, memo: p.memo, lines: p.lines }))
            }
            "entries.void" => {
                let p: IdDate = params(p)?;
                self.with(|s| s.void_entry(p.id, p.date.unwrap_or_else(today)))
            }

            // ---------------------------------------------------- bank-feed inbox
            "inbox.list" => {
                let p: MaybeAccount = params(p)?;
                self.with(|s| s.inbox(p.account_id))
            }
            "inbox.reviewed" => {
                let p: MaybeAccount = params(p)?;
                self.with(|s| {
                    s.reviewed(p.account_id, 200)?
                        .into_iter()
                        .map(|txn| {
                            // Where a posted transaction was filed: every line but the bank's own.
                            let categories = match (txn.status, txn.entry_id) {
                                (BankTxnStatus::Posted, Some(entry)) => {
                                    s.entry(entry)?.lines.iter().filter(|l| Some(l.id) != txn.line_id).map(|l| l.account_id).collect()
                                }
                                _ => vec![],
                            };
                            Ok(ReviewedView { txn, categories })
                        })
                        .collect::<Result<Vec<_>, StoreError>>()
                })
            }
            "inbox.count" => self.with(|s| s.inbox_count()),
            "inbox.candidates" => {
                let p: Id = params(p)?;
                self.with(|s| {
                    let t = s.bank_txn(p.id)?;
                    s.match_candidates(&t)
                })
            }
            "inbox.accept" => {
                let p: Accept = params(p)?;
                self.with(|s| {
                    let t = s.bank_txn(p.id)?;
                    let entry = s.accept(p.id, p.account_id, p.memo)?;
                    if p.remember {
                        remember_rule(s, &t, p.account_id)?;
                    }
                    Ok(entry)
                })
            }
            "inbox.split" => {
                let p: AcceptSplit = params(p)?;
                self.with(|s| s.accept_split(p.id, &p.splits, p.memo))
            }
            "inbox.match" => {
                let p: MatchLine = params(p)?;
                self.with(|s| s.match_txn(p.id, p.line_id))
            }
            "inbox.exclude" => {
                let p: Ids = params(p)?;
                self.with(|s| p.ids.iter().try_for_each(|id| s.exclude(*id)))
            }
            "inbox.undo" => {
                let p: Id = params(p)?;
                self.with(|s| s.undo_review(p.id))
            }
            "inbox.accept_suggested" => {
                let p: Ids = params(p)?;
                self.with(|s| accept_suggested(s, &p.ids))
            }

            // ---------------------------------------------------- file import
            "import.csv_preview" => {
                let p: Content = params(p)?;
                csv_import::preview(p.content.as_bytes(), 8).map_err(|e| ApiError::new("rule", e.to_string())).and_then(ok)
            }
            "import.csv" => {
                let p: CsvImport = params(p)?;
                let txns = csv_import::parse(p.content.as_bytes(), &p.mapping).map_err(|e| ApiError::new("rule", e.to_string()))?;
                self.with(|s| s.import_feed(p.account_id, &tally_core::bank::FeedBatch { added: txns, ..Default::default() }))
            }
            "import.ofx_preview" => {
                let p: Content = params(p)?;
                let statements = ofx::parse(p.content.as_bytes()).map_err(|e| ApiError::new("rule", e.to_string()))?;
                ok(statements
                    .into_iter()
                    .map(|st| OfxPreview {
                        count: st.transactions.len(),
                        from: st.transactions.iter().map(|t| t.date).min(),
                        to: st.transactions.iter().map(|t| t.date).max(),
                        account: st.account,
                    })
                    .collect::<Vec<_>>())
            }
            "import.ofx" => {
                let p: OfxImport = params(p)?;
                let mut statements = ofx::parse(p.content.as_bytes()).map_err(|e| ApiError::new("rule", e.to_string()))?;
                if p.index >= statements.len() {
                    return Err(ApiError::new("bad_params", "no such statement in that file"));
                }
                let st = statements.swap_remove(p.index);
                self.with(|s| s.import_feed(p.account_id, &tally_core::bank::FeedBatch { added: st.transactions, ..Default::default() }))
            }

            // ---------------------------------------------------- rules
            "rules.list" => self.with(|s| s.rules()),
            "rules.create" => {
                let p: NewRule = params(p)?;
                self.with(|s| s.create_rule(&p))
            }
            "rules.update" => {
                let p: UpdateRule = params(p)?;
                self.with(|s| s.update_rule(p.id, &p.rule))
            }
            "rules.delete" => {
                let p: Id = params(p)?;
                self.with(|s| s.delete_rule(p.id))
            }

            // ---------------------------------------------------- bank connections
            m if m.starts_with("bank.") => self.bank(m, p).await,

            // ---------------------------------------------------- reconciliation
            "reconcile.active" => {
                let p: AccountOnly = params(p)?;
                self.with(|s| s.active_reconciliation(p.account_id))
            }
            "reconcile.history" => {
                let p: AccountOnly = params(p)?;
                self.with(|s| s.reconciliations(p.account_id))
            }
            "reconcile.start" => {
                let p: StartRec = params(p)?;
                self.with(|s| s.start_reconciliation(p.account_id, p.statement_date, p.statement_balance))
            }
            "reconcile.candidates" => {
                let p: Id = params(p)?;
                self.with(|s| s.reconcile_candidates(p.id))
            }
            "reconcile.toggle" => {
                let p: Toggle = params(p)?;
                self.with(|s| s.set_cleared(p.id, p.line_id, p.checked))
            }
            "reconcile.toggle_many" => {
                let p: ToggleMany = params(p)?;
                self.with(|s| p.line_ids.iter().try_for_each(|line| s.set_cleared(p.id, *line, p.checked)))
            }
            "reconcile.summary" => {
                let p: Id = params(p)?;
                self.with(|s| s.reconciliation_summary(p.id))
            }
            "reconcile.finish" => {
                let p: Id = params(p)?;
                self.with(|s| s.finish_reconciliation(p.id))
            }
            "reconcile.cancel" => {
                let p: Id = params(p)?;
                self.with(|s| s.cancel_reconciliation(p.id))
            }
            "reconcile.undo_last" => {
                let p: AccountOnly = params(p)?;
                self.with(|s| s.undo_last_reconciliation(p.account_id))
            }

            // ---------------------------------------------------- contacts
            "contacts.list" => self.with(|s| s.contacts()),
            "contacts.get" => {
                let p: Id = params(p)?;
                self.with(|s| s.contact(p.id))
            }
            "contacts.create" => {
                let p: NewContact = params(p)?;
                self.with(|s| s.create_contact(&p))
            }
            "contacts.update" => {
                let p: UpdateContact = params(p)?;
                self.with(|s| s.update_contact(p.id, &p.contact))
            }
            "contacts.archive" => {
                let p: Archive = params(p)?;
                self.with(|s| s.set_contact_archived(p.id, p.archived))
            }

            // ---------------------------------------------------- invoices
            "invoices.list" => self.with(|s| s.invoices(today())),
            "invoices.get" => {
                let p: Id = params(p)?;
                self.with(|s| s.invoice(p.id, today()))
            }
            "invoices.create" => {
                let p: InvoiceDraft = params(p)?;
                self.with(|s| s.create_invoice(&p))
            }
            "invoices.update" => {
                let p: UpdateInvoice = params(p)?;
                self.with(|s| s.update_invoice(p.id, &p.draft))
            }
            "invoices.delete" => {
                let p: Id = params(p)?;
                self.with(|s| s.delete_draft_invoice(p.id))
            }
            "invoices.finalize" => {
                let p: Id = params(p)?;
                self.with(|s| s.finalize_invoice(p.id))
            }
            "invoices.void" => {
                let p: IdDate = params(p)?;
                self.with(|s| s.void_invoice(p.id, p.date.unwrap_or_else(today)))
            }

            // ---------------------------------------------------- bills & payments
            "bills.list" => self.with(|s| s.bills(today())),
            "bills.get" => {
                let p: Id = params(p)?;
                self.with(|s| {
                    let (summary, lines, payments) = s.bill(p.id, today())?;
                    Ok(json!({ "summary": summary, "lines": lines, "payments": payments }))
                })
            }
            "bills.create" => {
                let p: NewBill = params(p)?;
                self.with(|s| s.create_bill(&p))
            }
            "bills.void" => {
                let p: IdDate = params(p)?;
                self.with(|s| s.void_bill(p.id, p.date.unwrap_or_else(today)))
            }
            "payments.receive" => {
                let p: NewPayment = params(p)?;
                self.with(|s| s.receive_payment(&p))
            }
            "payments.pay" => {
                let p: NewPayment = params(p)?;
                self.with(|s| s.pay_bills(&p))
            }
            "payments.list" => {
                let p: PaymentsQuery = params(p)?;
                let to = p.to.unwrap_or_else(today);
                let from = p.from.unwrap_or(NaiveDate::MIN);
                self.with(|s| s.payments(p.kind, p.contact_id, from, to))
            }
            "payments.void" => {
                let p: IdDate = params(p)?;
                self.with(|s| s.void_payment(p.id, p.date.unwrap_or_else(today)))
            }

            _ => Err(ApiError::new("unknown_method", format!("no such method: {method}"))),
        }
    }

    // ------------------------------------------------------------ books

    fn create_book(&self, path: &Path, company: &str) -> ApiResult<Value> {
        if company.trim().is_empty() {
            return Err(ApiError::new("rule", "give your business a name"));
        }
        if path.exists() {
            return Err(ApiError::new("rule", format!("{} already exists", path.display())));
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| ApiError::new("io", e.to_string()))?;
        }
        let mut store = Store::open(path)?;
        store.seed_default_chart()?;
        store.set_pref("company.name", Some(company.trim()))?;
        self.install(store, path)
    }

    fn open_book(&self, path: &Path) -> ApiResult<Value> {
        if !path.exists() {
            return Err(ApiError::new("rule", format!("{} doesn't exist", path.display())));
        }
        let store = Store::open(path)?;
        self.install(store, path)
    }

    fn install(&self, store: Store, path: &Path) -> ApiResult<Value> {
        let name = store.pref("company.name")?.unwrap_or_else(|| "My Business".into());
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        *self.lock() = Some(OpenBook { store, path: path.clone() });
        let mut recent = self.recent();
        recent.retain(|r| r.path != path);
        recent.insert(0, RecentBook { path: path.clone(), name: name.clone(), last_opened: Local::now().to_rfc3339() });
        recent.truncate(10);
        self.save_recent(&recent);
        ok(json!({ "path": path, "company_name": name }))
    }

    fn recent(&self) -> Vec<RecentBook> {
        std::fs::read(self.config_dir.join("recent.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Vec<RecentBook>>(&b).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.path.exists())
            .collect()
    }

    fn save_recent(&self, recent: &[RecentBook]) {
        // Best effort: losing the recent list is an inconvenience, not data loss.
        let _ = std::fs::create_dir_all(&self.config_dir);
        if let Ok(json) = serde_json::to_vec_pretty(recent) {
            let _ = std::fs::write(self.config_dir.join("recent.json"), json);
        }
    }

    fn backup(&self) -> ApiResult<Value> {
        let guard = self.lock();
        let book = guard.as_ref().ok_or_else(|| ApiError::new("no_book", "no book is open"))?;
        let dir = book.path.parent().unwrap_or(Path::new(".")).join("Backups");
        std::fs::create_dir_all(&dir).map_err(|e| ApiError::new("io", e.to_string()))?;
        let stem = book.path.file_stem().and_then(|s| s.to_str()).unwrap_or("book");
        let dest = dir.join(format!("{stem} {}.tallybook", Local::now().format("%Y-%m-%d %H%M%S")));
        book.store.backup_to(&dest)?;
        ok(dest)
    }
}

/// `~/Documents/Tallybook`, or `$TALLYBOOK_DIR` when set (development, tests).
pub fn default_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("TALLYBOOK_DIR") {
        return PathBuf::from(dir);
    }
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join("Documents").join("Tallybook")
}

/// After the user categorizes a transaction with "remember", create a
/// contains-rule keyed on the payee so next month's lands pre-categorized.
fn remember_rule(s: &mut Store, t: &BankTxn, account_id: AccountId) -> Result<(), StoreError> {
    let pattern = t.merchant.clone().unwrap_or_else(|| tally_core::bank::normalize_description(&t.description));
    if pattern.trim().len() < 3 {
        return Ok(());
    }
    let exists = s.rules()?.iter().any(|r| r.pattern.eq_ignore_ascii_case(pattern.trim()));
    if exists {
        return Ok(());
    }
    s.create_rule(&NewRule {
        name: pattern.trim().to_string(),
        priority: 100,
        op: tally_core::rules::MatchOp::Contains,
        pattern: pattern.trim().to_string(),
        direction: if t.amount.0 > 0 { tally_core::rules::Direction::Inflow } else { tally_core::rules::Direction::Outflow },
        amount_min: None,
        amount_max: None,
        bank_account_id: None,
        target_account_id: account_id,
        rename_to: None,
        auto_post: false,
    })?;
    Ok(())
}

#[derive(Serialize)]
struct BatchResult {
    accepted: usize,
    skipped: usize,
}

/// "Accept all": applies each transaction's own suggestion, skipping ones without.
fn accept_suggested(s: &mut Store, ids: &[BankTxnId]) -> Result<BatchResult, StoreError> {
    let inbox = s.inbox(None)?;
    let mut r = BatchResult { accepted: 0, skipped: 0 };
    for id in ids {
        let Some(item) = inbox.iter().find(|i| i.txn.id == *id) else {
            r.skipped += 1;
            continue;
        };
        let done = match &item.suggestion {
            Some(Suggestion::Match { line_id, .. }) => s.match_txn(*id, *line_id).is_ok(),
            Some(Suggestion::Rule { account_id, rename_to, .. }) => s.accept(*id, *account_id, rename_to.clone()).is_ok(),
            Some(Suggestion::History { account_id }) => s.accept(*id, *account_id, None).is_ok(),
            None => false,
        };
        if done { r.accepted += 1 } else { r.skipped += 1 }
    }
    Ok(r)
}

#[cfg(test)]
mod tests;
