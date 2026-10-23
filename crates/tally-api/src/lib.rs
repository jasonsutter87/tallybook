//! Tallybook's application layer.
//!
//! The UI talks to exactly one function, [`App::call`]: a method name such as
//! `"invoices.create"` plus JSON params, returning JSON. The desktop shell
//! (Tauri) and the browser dev server both just forward to it, so the two can
//! never drift apart.
//!
//! Money crosses the boundary as integer cents; dates as `YYYY-MM-DD`;
//! budget months as `YYYY-MM`.
//!
//! The full method list is the `match` in [`App::call`], grouped under
//! `// ---- section` dividers. Each method's params are a `#[derive(Deserialize)]`
//! struct below (or a store type taken whole); missing `Option` fields take
//! the defaults noted beside them, usually "today". A book has one or two
//! sides (`"business"`, `"household"`); methods that take an optional `scope`
//! cover the whole book when it is absent, unless noted otherwise.

mod books;
mod cloud;
mod dashboard;
mod demo;
mod license;
mod pdf;
pub mod secrets;
mod sync;
mod tax_api;
mod tax_pdf;

use chrono::{Local, NaiveDate};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use tally_core::rules::RuleId;
use tally_core::{AccountId, AccountSubtype, Cents, EntryLine, Flex, NewEntry, Scope};
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
            StoreError::Locked => ApiError::new("locked", "this book is encrypted; enter its recovery key to open it"),
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

pub(crate) struct OpenBook {
    pub(crate) store: Store,
    pub(crate) path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RecentBook {
    path: PathBuf,
    name: String,
    last_opened: String,
    /// Computed when listing, from the file header.
    #[serde(default)]
    encrypted: bool,
}

pub struct App {
    book: Mutex<Option<OpenBook>>,
    secrets: Arc<dyn Secrets>,
    /// Where `recent.json` lives (the OS app-config dir).
    config_dir: PathBuf,
    /// How deleted books leave the disk: the OS Trash, or plain removal in tests.
    remove_file: fn(&Path) -> std::io::Result<()>,
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
struct ScopedRange {
    from: NaiveDate,
    to: NaiveDate,
    /// One side of the book; all of it when absent.
    scope: Option<Scope>,
}

#[derive(Deserialize)]
struct AsOf {
    as_of: Option<NaiveDate>,
    scope: Option<Scope>,
}

/// `dashboard`: which side to show; the book's main side when absent.
#[derive(Deserialize)]
struct MaybeScope {
    scope: Option<Scope>,
}

#[derive(Deserialize)]
struct CreateBook {
    path: PathBuf,
    company_name: String,
    /// Which sides to keep; business only when absent.
    #[serde(default)]
    sides: Option<Vec<Scope>>,
}

/// `book.add_side`: adding a side the book already has is a no-op.
#[derive(Deserialize)]
struct AddSide {
    scope: Scope,
}

/// `accounts.set_flex`: how far a household category can bend
/// ("fixed", "usage", "lifestyle"); null clears it. Household spending only.
#[derive(Deserialize)]
struct SetFlex {
    id: AccountId,
    flex: Option<Flex>,
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
    /// Defaults to the book's main side.
    scope: Option<Scope>,
    flex: Option<Flex>,
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
    /// Who the money was for (household books).
    #[serde(default)]
    person_id: Option<PersonId>,
    /// Who was paid, or who paid (business books; feeds 1099s).
    #[serde(default)]
    payee_id: Option<i64>,
}

/// `entries.set_person`: tag an entry as being for someone; null untags it.
#[derive(Deserialize)]
struct SetPerson {
    id: tally_core::journal::EntryId,
    person_id: Option<PersonId>,
}

/// `people.update`: replaces the person's name and color.
#[derive(Deserialize)]
struct UpdatePerson {
    id: PersonId,
    person: NewPerson,
}

/// `budget.month` and `household.*`: a `YYYY-MM` month; the current one when absent.
#[derive(Deserialize)]
struct MonthParam {
    month: Option<String>,
}

/// `budget.set`: sets one line of `month`'s plan. Plans carry forward, so
/// this also changes later months that had no plan of their own.
#[derive(Deserialize)]
struct SetBudget {
    month: String,
    line: BudgetLine,
}

/// `budget.remove`: drops one line from `month`'s plan onward.
/// `target` is `{"kind": "person" | "account", "id": n}`.
#[derive(Deserialize)]
struct RemoveBudget {
    month: String,
    target: BudgetTarget,
}

/// `recurring.update`: replaces every field of the bill.
#[derive(Deserialize)]
struct UpdateBill {
    id: RecurringBillId,
    bill: NewRecurringBill,
}

/// `goals.update`: replaces every field of the goal.
#[derive(Deserialize)]
struct UpdateGoal {
    id: GoalId,
    goal: NewGoal,
}

/// `goals.contribute`: moves `amount` from account `from` into the goal's
/// savings account, tagged to the goal. Negative takes money back out.
#[derive(Deserialize)]
struct Contribute {
    id: GoalId,
    from: AccountId,
    amount: Cents,
    /// Today when absent.
    date: Option<NaiveDate>,
}

/// `entries.set_goal`: counts an existing entry toward a goal; null untags it.
/// The entry must touch the goal's savings account.
#[derive(Deserialize)]
struct SetGoal {
    id: tally_core::journal::EntryId,
    goal_id: Option<GoalId>,
}

/// `import.ledger`: the Ledger SQLite file to bring over.
/// Safe to repeat: only entries not imported before are added.
#[derive(Deserialize)]
struct LedgerImportParams {
    path: PathBuf,
    /// The household account Ledger's money lands in; a new
    /// "Ledger import" account when absent.
    money_account_id: Option<AccountId>,
}

/// `recurring.set_active`: an inactive bill stops producing due dates.
#[derive(Deserialize)]
struct SetActive {
    id: i64,
    active: bool,
}

/// `recurring.due`: the window of due dates to list. Defaults: from a week
/// ago through the end of next month.
#[derive(Deserialize)]
struct DueQuery {
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
}

/// `recurring.dismiss_suggestion`: "Not a bill". Send the suggestion's
/// `match_text`; it is remembered so the payee isn't suggested again.
#[derive(Deserialize)]
struct DismissSuggestion {
    match_text: String,
}

/// `recurring.mark`: overrides one due date by hand, e.g. paid in cash.
/// `due_date` must be one of the bill's actual due dates.
#[derive(Deserialize)]
struct MarkDue {
    bill_id: RecurringBillId,
    due_date: NaiveDate,
    /// "paid", "skipped", or null to go back to automatic.
    status: Option<DueState>,
    /// The entry that paid it, when there is one.
    entry_id: Option<tally_core::journal::EntryId>,
}

/// `budget.set_available`: what there is to work with in `month`; null clears it.
#[derive(Deserialize)]
struct SetAvailable {
    month: String,
    amount: Option<Cents>,
}

#[derive(Deserialize)]
struct Accept {
    id: BankTxnId,
    account_id: AccountId,
    memo: Option<String>,
    /// Also create a rule so future transactions like this auto-categorize.
    #[serde(default)]
    remember: bool,
    /// Who was paid (for 1099s) or who paid you.
    #[serde(default)]
    payee_id: Option<i64>,
    /// Who the money was for (household books).
    #[serde(default)]
    person_id: Option<PersonId>,
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
    encrypted: bool,
    today: NaiveDate,
    /// Sides this book keeps; empty when no book is open.
    sides: Vec<Scope>,
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
        App { book: Mutex::new(None), secrets, config_dir: config_dir.into(), remove_file: books::move_to_trash }
    }

    /// Replaces the Trash with permanent removal (tests, headless servers).
    pub fn with_file_remover(mut self, remove: fn(&Path) -> std::io::Result<()>) -> App {
        self.remove_file = remove;
        self
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
                let (path, name, encrypted, sides) = match guard.as_ref() {
                    Some(b) => (Some(b.path.clone()), b.store.pref("company.name")?, b.store.is_encrypted(), b.store.sides()?),
                    None => (None, None, false, vec![]),
                };
                ok(BookStatus { open: guard.is_some(), path, company_name: name, encrypted, today: today(), sides })
            }
            "book.create" => {
                let p: CreateBook = params(p)?;
                let sides = p.sides.unwrap_or_else(|| vec![Scope::Business]);
                if sides.contains(&Scope::Business) {
                    self.require(license::Tier::Business)?;
                }
                self.create_book(&p.path, &p.company_name, &sides)
            }
            "book.create_demo" => {
                let p: OpenPath = params(p)?;
                self.create_book(&p.path, "Northwind Studio", &[Scope::Business])?;
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
            // {scope} -> null. Adds a side and its default chart.
            "book.add_side" => {
                let p: AddSide = params(p)?;
                if p.scope == Scope::Business {
                    self.require(license::Tier::Business)?;
                }
                self.with(|s| s.add_side(p.scope))
            }
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
            "dashboard" => {
                let p: MaybeScope = params(p)?;
                self.with(|s| {
                    let scope = match p.scope {
                        Some(scope) => scope,
                        None => main_side(s)?,
                    };
                    dashboard::build(s, today(), scope)
                })
            }
            "reports.pnl" => {
                let p: ScopedRange = params(p)?;
                self.with(|s| s.profit_and_loss_in(p.from, p.to, p.scope))
            }
            "reports.balance_sheet" => {
                let p: AsOf = params(p)?;
                self.with(|s| s.balance_sheet_in(p.as_of.unwrap_or_else(today), p.scope))
            }
            "reports.trial_balance" => {
                let p: AsOf = params(p)?;
                self.with(|s| s.trial_balance_in(p.as_of.unwrap_or_else(today), p.scope))
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
                    let scope = match p.scope {
                        Some(scope) => scope,
                        None => main_side(s)?,
                    };
                    let id = s.create_account(&NewAccount { code: p.code, name: p.name, subtype: p.subtype, parent_id: p.parent_id, scope, flex: p.flex })?;
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
            // {id, flex} -> null.
            "accounts.set_flex" => {
                let p: SetFlex = params(p)?;
                self.with(|s| s.set_flex(p.id, p.flex))
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
                self.with(|s| {
                    let id = s.post_entry(&NewEntry { date: p.date, memo: p.memo, lines: p.lines })?;
                    if p.person_id.is_some() {
                        s.set_entry_person(id, p.person_id)?;
                    }
                    if p.payee_id.is_some() {
                        s.set_entry_payee(id, p.payee_id)?;
                    }
                    Ok(id)
                })
            }
            // {id, person_id} -> null.
            "entries.set_person" => {
                let p: SetPerson = params(p)?;
                self.with(|s| s.set_entry_person(p.id, p.person_id))
            }

            // ---------------------------------------------------- people & budgets
            // Household side only. People are who money was for; budgets are
            // monthly plans of ceilings (spend at most) and floors (at least).

            // null -> [Person], archived ones last.
            "people.list" => self.with(|s| s.people()),
            // {name, color?} -> new person id.
            "people.create" => {
                let p: NewPerson = params(p)?;
                self.with(|s| s.create_person(&p))
            }
            // {id, person} -> null.
            "people.update" => {
                let p: UpdatePerson = params(p)?;
                self.with(|s| s.update_person(p.id, &p.person))
            }
            // {id, archived} -> null. Archived people drop out of later plans.
            "people.archive" => {
                let p: Archive = params(p)?;
                self.with(|s| s.set_person_archived(p.id, p.archived))
            }
            // {month?} -> MonthBudget: the plan in effect, progress so far, and the
            // pace projection, judged as of today.
            "budget.month" => {
                let p: MonthParam = params(p)?;
                let month = p.month.unwrap_or_else(|| month_of(today()));
                self.with(|s| s.month_budget(&month, today()))
            }
            // {month, line} -> budget line id.
            "budget.set" => {
                let p: SetBudget = params(p)?;
                self.with(|s| s.set_budget_line(&p.month, &p.line))
            }
            // {month, target} -> null.
            "budget.remove" => {
                let p: RemoveBudget = params(p)?;
                self.with(|s| s.remove_budget_line(&p.month, p.target))
            }

            // ---------------------------------------------------- recurring bills
            // null -> [RecurringBill], each with its next due date.
            "recurring.list" => self.with(|s| s.recurring_bills(today())),
            // NewRecurringBill -> new bill id.
            "recurring.create" => {
                let p: NewRecurringBill = params(p)?;
                self.with(|s| s.create_recurring_bill(&p))
            }
            // {id, bill} -> null.
            "recurring.update" => {
                let p: UpdateBill = params(p)?;
                self.with(|s| s.update_recurring_bill(p.id, &p.bill))
            }
            // {id, active} -> null.
            "recurring.set_active" => {
                let p: SetActive = params(p)?;
                self.with(|s| s.set_recurring_bill_active(p.id, p.active))
            }
            // {from?, to?} -> [DueDate], each paid / overdue / upcoming / etc.
            "recurring.due" => {
                // Default: the past week through the end of next month.
                let p: DueQuery = params(p)?;
                let t = today();
                let from = p.from.unwrap_or(t - chrono::Days::new(7));
                let to = p.to.unwrap_or_else(|| month_range(&month_of(t + chrono::Months::new(1))).map(|r| r.1).unwrap_or(t));
                self.with(|s| s.due_dates(from, to, t))
            }
            // {bill_id, due_date, status, entry_id?} -> null.
            "recurring.mark" => {
                let p: MarkDue = params(p)?;
                self.with(|s| s.mark_due_date(p.bill_id, p.due_date, p.status, p.entry_id))
            }
            // null -> [BillSuggestion]: repeat payments that look like bills,
            // detected from the books and the Review inbox. At most 50.
            "recurring.suggestions" => self.with(|s| s.bill_suggestions(today())),
            // {match_text} -> null.
            "recurring.dismiss_suggestion" => {
                let p: DismissSuggestion = params(p)?;
                self.with(|s| s.dismiss_bill_suggestion(&p.match_text))
            }

            // ---------------------------------------------------- goals & insights
            // null -> [Goal], each with saved so far and a projection.
            "goals.list" => self.with(|s| s.goals(today())),
            // NewGoal -> new goal id. The goal's account must be a household asset.
            "goals.create" => {
                let p: NewGoal = params(p)?;
                self.with(|s| s.create_goal(&p))
            }
            // {id, goal} -> null.
            "goals.update" => {
                let p: UpdateGoal = params(p)?;
                self.with(|s| s.update_goal(p.id, &p.goal))
            }
            // {id, archived} -> null.
            "goals.archive" => {
                let p: Archive = params(p)?;
                self.with(|s| s.set_goal_archived(p.id, p.archived))
            }
            // {id, from, amount, date?} -> {entry_id, finished}; finished means this one reached the target.
            "goals.contribute" => {
                let p: Contribute = params(p)?;
                self.with(|s| s.contribute_to_goal(p.id, p.from, p.amount, p.date.unwrap_or_else(today)))
            }
            // {id, goal_id} -> null.
            "entries.set_goal" => {
                let p: SetGoal = params(p)?;
                self.with(|s| s.set_entry_goal(p.id, p.goal_id))
            }
            // {month?} -> alerts: [Alert] (near or over budget, unpaid bills);
            // opportunities: Opportunities (spending by flex, where it could bend);
            // score: BudgetScore (a 300-850 style score with its factors);
            // wins: [Win] (true, specific good news).
            "household.alerts" | "household.opportunities" | "household.score" | "household.wins" => {
                let p: MonthParam = params(p)?;
                let month = p.month.unwrap_or_else(|| month_of(today()));
                match method {
                    "household.alerts" => self.with(|s| s.alerts(&month, today())),
                    "household.opportunities" => self.with(|s| s.opportunities(&month)),
                    "household.score" => self.with(|s| s.budget_score(&month, today())),
                    _ => self.with(|s| s.wins(&month, today())),
                }
            }
            // {month, amount} -> null.
            "budget.set_available" => {
                let p: SetAvailable = params(p)?;
                self.with(|s| s.set_available(&p.month, p.amount))
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
                    if p.payee_id.is_some() {
                        s.set_entry_payee(entry, p.payee_id)?;
                    }
                    if p.person_id.is_some() {
                        s.set_entry_person(entry, p.person_id)?;
                    }
                    if p.remember {
                        remember_rule(s, &t, p.account_id, p.person_id)?;
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

            // {path} -> LedgerPreview: counts and date range, without importing.
            "import.ledger_preview" => {
                let p: OpenPath = params(p)?;
                ok(Store::preview_ledger(&p.path)?)
            }
            // {path, money_account_id?} -> LedgerImport: what was added and skipped,
            // plus notes on anything that couldn't come over as-is.
            "import.ledger" => {
                let p: LedgerImportParams = params(p)?;
                self.with(|s| s.import_ledger(&p.path, p.money_account_id))
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
            m if m.starts_with("tax.") => self.tax(m, p).await,
            m if m.starts_with("license.") => self.license(m, p).await,
            m if m.starts_with("cloud.") => self.cloud(m, p).await,
            "book.delete" | "book.unlock" | "book.encrypt" | "book.decrypt" | "book.recovery_key" | "book.delete_plain_backups" => {
                self.books(method, p).await
            }

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

    fn create_book(&self, path: &Path, company: &str, sides: &[Scope]) -> ApiResult<Value> {
        if company.trim().is_empty() {
            return Err(ApiError::new("rule", "give your book a name"));
        }
        if sides.is_empty() {
            return Err(ApiError::new("rule", "a book keeps a business, a household, or both"));
        }
        if path.exists() {
            return Err(ApiError::new("rule", format!("{} already exists", path.display())));
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| ApiError::new("io", e.to_string()))?;
        }
        let mut store = Store::open(path)?;
        for side in sides {
            store.add_side(*side)?;
        }
        store.set_pref("company.name", Some(company.trim()))?;
        self.install(store, path)
    }

    fn open_book(&self, path: &Path) -> ApiResult<Value> {
        if !path.exists() {
            return Err(ApiError::new("rule", format!("{} doesn't exist", path.display())));
        }
        let store = self.open_any(path)?;
        self.install(store, path)
    }

    pub(crate) fn install(&self, store: Store, path: &Path) -> ApiResult<Value> {
        let name = store.pref("company.name")?.unwrap_or_else(|| "My Business".into());
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        *self.lock() = Some(OpenBook { store, path: path.clone() });
        let mut recent = self.recent();
        recent.retain(|r| r.path != path);
        recent.insert(0, RecentBook { path: path.clone(), name: name.clone(), last_opened: Local::now().to_rfc3339(), encrypted: false });
        recent.truncate(10);
        self.save_recent(&recent);
        ok(json!({ "path": path, "company_name": name }))
    }

    pub(crate) fn recent(&self) -> Vec<RecentBook> {
        std::fs::read(self.config_dir.join("recent.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Vec<RecentBook>>(&b).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.path.exists())
            .map(|mut r| {
                r.encrypted = tally_store::crypto::is_encrypted_file(&r.path).unwrap_or(false);
                r
            })
            .collect()
    }

    pub(crate) fn save_recent(&self, recent: &[RecentBook]) {
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
        let stamp = Local::now().format("%Y-%m-%d %H%M%S").to_string();
        // Two backups in the same second get " (2)", " (3)"… rather than failing.
        let dest = (1..)
            .map(|n| dir.join(if n == 1 { format!("{stem} {stamp}.tallybook") } else { format!("{stem} {stamp} ({n}).tallybook") }))
            .find(|p| !p.exists())
            .expect("an unused name exists");
        book.store.backup_to(&dest)?;
        ok(dest)
    }
}

/// The side a book is mostly about: business when it keeps one.
pub(crate) fn main_side(s: &Store) -> Result<Scope, StoreError> {
    Ok(s.sides()?.first().copied().unwrap_or_default())
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
fn remember_rule(s: &mut Store, t: &BankTxn, account_id: AccountId, person_id: Option<PersonId>) -> Result<(), StoreError> {
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
        person_id,
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
            Some(Suggestion::Rule { account_id, rename_to, .. }) => accept_for(s, *id, *account_id, rename_to.clone(), item.person_id)?,
            Some(Suggestion::History { account_id }) => accept_for(s, *id, *account_id, None, item.person_id)?,
            None => false,
        };
        if done { r.accepted += 1 } else { r.skipped += 1 }
    }
    Ok(r)
}

/// Accepts into a category and tags who it was for; `false` if it didn't post.
fn accept_for(s: &mut Store, id: BankTxnId, account_id: AccountId, memo: Option<String>, person: Option<PersonId>) -> Result<bool, StoreError> {
    let Ok(entry) = s.accept(id, account_id, memo) else { return Ok(false) };
    if person.is_some() {
        s.set_entry_person(entry, person)?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests;
