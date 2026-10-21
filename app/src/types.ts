// Mirrors the JSON produced by crates/tally-api. Money is always integer
// cents; dates are "YYYY-MM-DD" strings.

export type Cents = number;
export type ISODate = string;

export type AccountKind = "asset" | "liability" | "equity" | "income" | "expense";
export type AccountSubtype =
  | "bank"
  | "accounts_receivable"
  | "other_current_asset"
  | "fixed_asset"
  | "credit_card"
  | "accounts_payable"
  | "other_current_liability"
  | "long_term_liability"
  | "equity"
  | "retained_earnings"
  | "income"
  | "other_income"
  | "cost_of_goods_sold"
  | "expense"
  | "other_expense";

export interface BankLink {
  id: number;
  connection_id: number;
  external_account_id: string;
  ledger_account_id: number;
  name: string;
  mask: string | null;
  bank_balance: Cents | null;
  start_date: ISODate;
}

/** Which side of the book an account belongs to. Each side balances on its own. */
export type Scope = "business" | "household";

/** How far a household spending category can bend. */
export type Flex = "fixed" | "usage" | "lifestyle";

export interface Account {
  id: number;
  code: string | null;
  name: string;
  subtype: AccountSubtype;
  parent_id: number | null;
  archived: boolean;
  scope: Scope;
  flex: Flex | null;
  kind: AccountKind;
  /** Natural sign: what you have (assets), owe (liabilities), earned/spent. */
  balance: Cents;
  link: BankLink | null;
  reconcilable: boolean;
}

export interface SubtypeInfo {
  value: AccountSubtype;
  kind: AccountKind;
}

export type Cleared = "uncleared" | "cleared" | "reconciled";

export interface RegisterRow {
  entry_id: number;
  line_id: number;
  date: ISODate;
  memo: string;
  /** Natural sign for the account. */
  amount: Cents;
  balance: Cents;
  cleared: Cleared;
  voided: boolean;
}

export interface PostedLine {
  id: number;
  account_id: number;
  /** Raw: positive debit, negative credit. */
  amount: Cents;
  memo: string | null;
  cleared: Cleared;
  reconciliation_id: number | null;
}

export interface Entry {
  id: number;
  date: ISODate;
  memo: string;
  source: string;
  reverses_id: number | null;
  voided_by_id: number | null;
  /** Customer or vendor this entry was with. */
  payee_id: number | null;
  /** Who the money was for (household books). */
  person_id: number | null;
  lines: PostedLine[];
}

export interface EntryLineInput {
  account_id: number;
  /** Raw: positive debit, negative credit. */
  amount: Cents;
  memo: string | null;
}

// ------------------------------------------------------------- dashboard

export interface MoneyAccount {
  id: number;
  name: string;
  subtype: AccountSubtype;
  balance: Cents;
  bank_balance: Cents | null;
  mask: string | null;
  linked: boolean;
}

export interface Period {
  income: Cents;
  expenses: Cents;
  net: Cents;
}

export interface Dashboard {
  today: ISODate;
  cash: Cents;
  credit_owed: Cents;
  receivable: Cents;
  payable: Cents;
  accounts: MoneyAccount[];
  this_month: Period;
  last_month: Period;
  cashflow: { month: string; income: Cents; expenses: Cents }[];
  top_expenses: { account_id: number; name: string; amount: Cents }[];
  inbox_count: number;
  overdue_invoices: { count: number; amount: Cents };
  bills_due_soon: { count: number; amount: Cents };
}

// ------------------------------------------------------------- reports

export interface ReportLine {
  account_id: number;
  code: string | null;
  name: string;
  subtype: AccountSubtype;
  amount: Cents;
}

export interface ProfitAndLoss {
  income: ReportLine[];
  cost_of_goods_sold: ReportLine[];
  expenses: ReportLine[];
  total_income: Cents;
  total_cost_of_goods_sold: Cents;
  gross_profit: Cents;
  total_expenses: Cents;
  net_income: Cents;
}

export interface BalanceSheet {
  assets: ReportLine[];
  liabilities: ReportLine[];
  equity: ReportLine[];
  retained_earnings: Cents;
  net_income: Cents;
  total_assets: Cents;
  total_liabilities: Cents;
  total_equity: Cents;
}

export interface TrialBalance {
  rows: { account_id: number; code: string | null; name: string; debit: Cents; credit: Cents }[];
  total_debits: Cents;
  total_credits: Cents;
}

export interface AgingBuckets {
  current: Cents;
  days_1_30: Cents;
  days_31_60: Cents;
  days_61_90: Cents;
  over_90: Cents;
}

export interface AgingReport {
  as_of: ISODate;
  rows: { contact_id: number; name: string; buckets: AgingBuckets }[];
  totals: AgingBuckets;
}

// ------------------------------------------------------------- bank feeds

export type Suggestion =
  | { kind: "match"; line_id: number; entry_id: number; date: ISODate; memo: string }
  | { kind: "rule"; rule_id: number; account_id: number; rename_to: string | null }
  | { kind: "history"; account_id: number };

export type BankTxnStatus = "new" | "posted" | "matched" | "excluded" | "removed";

export interface BankTxn {
  id: number;
  ledger_account_id: number;
  external_id: string;
  date: ISODate;
  /** Inflow-positive: money in is +, money out is −. */
  amount: Cents;
  description: string;
  merchant: string | null;
  category_hint: string | null;
  pending: boolean;
  status: BankTxnStatus;
  entry_id: number | null;
  line_id: number | null;
  rule_id: number | null;
}

export interface ReviewedTxn extends BankTxn {
  /** Accounts a posted transaction was filed to (more than one for a split). */
  categories: number[];
}

export interface InboxItem extends BankTxn {
  suggestion: Suggestion | null;
  /** Who it was probably for. */
  person_id: number | null;
}

export interface ImportSummary {
  added: number;
  updated: number;
  removed: number;
  skipped: number;
  auto_posted: number;
}

export interface Split {
  account_id: number;
  /** Same sign as the bank transaction; splits must sum to it. */
  amount: Cents;
  memo: string | null;
}

export type MatchOp = "contains" | "equals" | "starts_with";
export type Direction = "any" | "inflow" | "outflow";

export interface NewRule {
  name: string;
  priority: number;
  op: MatchOp;
  pattern: string;
  direction: Direction;
  amount_min: Cents | null;
  amount_max: Cents | null;
  bank_account_id: number | null;
  target_account_id: number;
  rename_to: string | null;
  auto_post: boolean;
  /** Who the money is for (household books). */
  person_id?: number | null;
}

export interface Rule extends NewRule {
  id: number;
}

export type DateFormat = "iso" | "month_day_year" | "day_month_year";

export interface CsvMapping {
  has_header: boolean;
  date_column: number;
  date_format: DateFormat;
  description_column: number;
  amount: { kind: "single"; column: number; negate: boolean } | { kind: "split"; debit: number; credit: number };
}

export interface CsvPreview {
  headers: string[];
  rows: string[][];
  mapping: CsvMapping | null;
}

export type FeedAccountKind = "depository" | "credit" | "loan" | "investment" | "other";

export interface FeedAccount {
  external_id: string;
  name: string;
  mask: string | null;
  kind: FeedAccountKind;
  current_balance: Cents | null;
  currency: string | null;
}

export interface OfxPreview {
  account: FeedAccount;
  count: number;
  from: ISODate | null;
  to: ISODate | null;
}

export type Provider = "plaid" | "simplefin" | "file";
export type ConnectionStatus = "ok" | "needs_relink" | "error" | "disconnected";

export interface Connection {
  id: number;
  provider: Provider;
  institution: string;
  external_id: string | null;
  secret_ref: string | null;
  cursor: string;
  status: ConnectionStatus;
  last_error: string | null;
  last_synced_at: string | null;
  links: BankLink[];
}

export interface PlaidConfig {
  configured: boolean;
  environment: "sandbox" | "production" | null;
  client_id_hint: string | null;
}

export interface HostedLink {
  link_token: string;
  url: string;
}

export type PollResult =
  | { status: "waiting" }
  | { status: "exited" }
  | { status: "linked"; connection_id: number; institution: string; accounts: FeedAccount[] };

export interface SyncReport {
  connection_id: number;
  institution: string;
  ok: boolean;
  error: string | null;
  imported: ImportSummary;
}

export interface OpeningSuggestion {
  date: ISODate;
  amount: Cents;
  bank_balance: Cents;
}

// ------------------------------------------------------------- reconciliation

export interface Reconciliation {
  id: number;
  account_id: number;
  statement_date: ISODate;
  statement_balance: Cents;
  status: "in_progress" | "completed";
}

export interface ReconcileCandidate {
  line_id: number;
  entry_id: number;
  date: ISODate;
  memo: string;
  amount: Cents;
  checked: boolean;
}

export interface ReconcileSummary {
  beginning_balance: Cents;
  cleared_total: Cents;
  cleared_balance: Cents;
  statement_balance: Cents;
  difference: Cents;
}

// ------------------------------------------------------------- sales & purchases

export type ContactKind = "customer" | "vendor" | "both";

export interface NewContact {
  kind: ContactKind;
  name: string;
  company: string | null;
  email: string | null;
  phone: string | null;
  address: string | null;
  notes: string | null;
}

export interface Contact extends NewContact {
  id: number;
  archived: boolean;
  open_balance: Cents;
  is_1099: boolean;
  w9_on_file: boolean;
  legal_name: string | null;
  tin_last4: string | null;
}

export type DocStatus = "draft" | "open" | "partial" | "paid" | "overdue" | "void";

export interface InvoiceSummary {
  id: number;
  number: string;
  customer_id: number;
  customer_name: string;
  issue_date: ISODate;
  due_date: ISODate;
  total: Cents;
  paid: Cents;
  balance: Cents;
  status: DocStatus;
}

export interface InvoiceLineInput {
  description: string;
  /** Thousandths: 1.5 hours = 1500. */
  quantity_milli: number;
  unit_price: Cents;
  account_id: number;
  taxable: boolean;
}

export interface InvoiceLine extends InvoiceLineInput {
  amount: Cents;
}

export interface AppliedPayment {
  payment_id: number;
  date: ISODate;
  amount: Cents;
  method: string;
  reference: string;
}

export interface PaymentSummary {
  id: number;
  kind: "received" | "made";
  contact_id: number;
  contact_name: string;
  date: ISODate;
  amount: Cents;
  account_id: number;
  method: string;
  reference: string;
  voided: boolean;
  /** Invoice ids (received) or bill ids (made). */
  document_ids: number[];
}

export interface InvoiceDetail extends InvoiceSummary {
  memo: string;
  tax_rate_bp: number;
  subtotal: Cents;
  tax: Cents;
  entry_id: number | null;
  lines: InvoiceLine[];
  payments: AppliedPayment[];
}

export interface InvoiceDraft {
  customer_id: number;
  issue_date: ISODate;
  due_date: ISODate;
  memo: string;
  /** Basis points: 8.25% = 825. */
  tax_rate_bp: number;
  lines: InvoiceLineInput[];
}

export interface BillLineInput {
  description: string;
  amount: Cents;
  account_id: number;
}

export interface NewBill {
  vendor_id: number;
  number: string;
  issue_date: ISODate;
  due_date: ISODate;
  memo: string;
  lines: BillLineInput[];
}

export interface BillSummary {
  id: number;
  number: string;
  vendor_id: number;
  vendor_name: string;
  issue_date: ISODate;
  due_date: ISODate;
  total: Cents;
  paid: Cents;
  balance: Cents;
  status: DocStatus;
  memo: string;
}

export interface BillDetail {
  summary: BillSummary;
  lines: BillLineInput[];
  payments: AppliedPayment[];
}

export interface NewPayment {
  contact_id: number;
  date: ISODate;
  /** Deposit-to (received) or paid-from (made). */
  account_id: number;
  method: string;
  reference: string;
  memo: string;
  applications: { document_id: number; amount: Cents }[];
}

// ------------------------------------------------------------- book & settings

export interface BookStatus {
  open: boolean;
  path: string | null;
  company_name: string | null;
  encrypted: boolean;
  today: ISODate;
  /** Sides this book keeps, business first. */
  sides: Scope[];
}

export interface RecentBook {
  path: string;
  name: string;
  last_opened: string;
  encrypted: boolean;
}

export interface EncryptResult {
  /** Shown once: TB1-XXXXX-… */
  recovery_key: string;
  /** Backups made before encrypting, still readable. */
  plain_backups: number;
}

export interface CloudStatus {
  encrypted: boolean;
  folder: string | null;
  git: string | null;
  git_has_remote: boolean;
  auto: boolean;
  keep: number;
  last: string | null;
  last_error: string | null;
  due: boolean;
  suggestions: { label: string; path: string }[];
}

export interface CloudBackupReport {
  folder: { ok: boolean; path: string | null; detail: string | null } | null;
  git: { ok: boolean; path: string | null; detail: string | null } | null;
}

export interface Settings {
  prefs: Record<string, string>;
  fiscal_year_start_month: number;
  lock_date: ISODate | null;
}

// ------------------------------------------------------------- tax center

export type FilingStatus = "single" | "married_joint" | "married_separate" | "head_of_household";

export type ScheduleCLine =
  | "l1_gross_receipts"
  | "l2_returns"
  | "l4_cost_of_goods"
  | "l6_other_income"
  | "l8_advertising"
  | "l9_car_truck"
  | "l10_commissions"
  | "l11_contract_labor"
  | "l13_depreciation"
  | "l14_employee_benefits"
  | "l15_insurance"
  | "l16a_mortgage_interest"
  | "l16b_other_interest"
  | "l17_legal"
  | "l18_office"
  | "l19_pension"
  | "l20a_rent_vehicles"
  | "l20b_rent_other"
  | "l21_repairs"
  | "l22_supplies"
  | "l23_taxes"
  | "l24a_travel"
  | "l24b_meals"
  | "l25_utilities"
  | "l26_wages"
  | "l27a_other"
  | "not_deductible";

export interface TaxYears {
  supported: number[];
  current: number;
  disclaimer: string;
}

export interface LineInfo {
  line: ScheduleCLine;
  /** Form line number, e.g. "24b"; "—" for not on Schedule C. */
  number: string;
  label: string;
  help: string;
}

export interface TaxMappingRow {
  account_id: number;
  code: string | null;
  name: string;
  kind: AccountKind;
  amount: Cents;
  line: ScheduleCLine;
  /** False = only Tallybook's suggestion so far. */
  confirmed: boolean;
}

export interface ScheduleCLineTotal {
  line: ScheduleCLine;
  number: string;
  label: string;
  booked: Cents;
  amount: Cents;
}

export interface ScheduleC {
  line1_gross_receipts: Cents;
  line2_returns: Cents;
  line3: Cents;
  line4_cogs: Cents;
  line5_gross_profit: Cents;
  line6_other_income: Cents;
  line7_gross_income: Cents;
  expenses: ScheduleCLineTotal[];
  line28_total_expenses: Cents;
  line29_tentative_profit: Cents;
  line30_home_office: Cents;
  line31_net_profit: Cents;
  meals_disallowed: Cents;
}

export interface ScheduleCResponse {
  report: { year: number; from: ISODate; to: ISODate; schedule: ScheduleC; rows: TaxMappingRow[]; unconfirmed: number };
  home_office_sqft: number;
  disclaimer: string;
}

export interface TaxProfile {
  filing_status: FilingStatus;
  method: "safe_harbor" | "current_year";
  other_income: Cents;
  w2_wages: Cents;
  withholding: Cents;
  last_year_tax: Cents | null;
  last_year_agi: Cents | null;
  /** Overrides the annualized projection when set. */
  projected_profit: Cents | null;
  home_office_sqft: number;
  /** Rough state rate in basis points (500 = 5%). */
  state_rate_bp: number;
}

export interface SeTax {
  net_earnings: Cents;
  social_security: Cents;
  medicare: Cents;
  total: Cents;
  deductible_half: Cents;
}

export interface TaxEstimateDetail {
  year: number;
  source: string;
  se: SeTax;
  additional_medicare: Cents;
  agi: Cents;
  standard_deduction: Cents;
  qbi_deduction: Cents;
  qbi_limited: boolean;
  taxable_income: Cents;
  income_tax: Cents;
  total_tax: Cents;
  withholding: Cents;
  to_pay: Cents;
  /** Basis points of business profit to set aside. */
  set_aside_bp: number;
  not_included: string[];
}

export type Jurisdiction = "federal" | "state";

export interface TaxPayment {
  id: number;
  tax_year: number;
  quarter: number;
  jurisdiction: Jurisdiction;
  date: ISODate;
  amount: Cents;
  account_id: number;
  entry_id: number;
  voided: boolean;
}

export interface QuarterPlan {
  quarter: number;
  due: ISODate;
  planned: Cents;
  paid_federal: Cents;
  paid_state: Cents;
  status: "paid" | "nothing_due" | "late" | "due_soon" | "upcoming";
}

export interface TaxEstimate {
  year: number;
  profile: TaxProfile;
  profit_to_date: Cents;
  profit_annualized: Cents;
  profit_used: Cents;
  profit_basis: string;
  estimate: TaxEstimateDetail;
  safe_harbor: Cents | null;
  ninety_percent_current: Cents;
  plan_annual: Cents;
  plan_method: "safe_harbor" | "current_year";
  quarters: QuarterPlan[];
  state_set_aside: Cents;
  payments: TaxPayment[];
  disclaimer: string;
}

export interface Payee1099 {
  contact_id: number;
  name: string;
  legal_name: string | null;
  is_1099: boolean;
  w9_on_file: boolean;
  tin_last4: string | null;
  paid_reportable: Cents;
  paid_by_card: Cents;
  over_threshold: boolean;
}

export interface Report1099 {
  year: number;
  threshold: Cents;
  rows: Payee1099[];
}

export interface ContactTaxInput {
  is_1099: boolean;
  w9_on_file: boolean;
  legal_name: string | null;
  tin_last4: string | null;
}

// ------------------------------------------------------------- household budgets

export interface Person {
  id: number;
  name: string;
  color: string;
  sort_order: number;
  archived: boolean;
}

/** Spend up to this (ceiling), or get at least this much in (floor). */
export type BudgetDirection = "ceiling" | "floor";
export type Zone = "under" | "in_range" | "trending_over" | "over";
export type Sentiment = "good" | "caution" | "alarm";

export type BudgetTarget = { kind: "person"; id: number } | { kind: "account"; id: number };

export interface BudgetStatus {
  zone: Zone;
  sentiment: Sentiment;
  /** Projected end-of-month total. */
  pace: Cents;
  /** 0..1 of the month gone. */
  elapsed: number;
  remaining: Cents;
  band: Cents;
}

export interface BudgetLineView {
  id: number;
  target: BudgetTarget;
  name: string;
  flex: Flex | null;
  amount: Cents;
  direction: BudgetDirection;
  tolerance_pct: number;
  progress: Cents;
  status: BudgetStatus;
  percent_used: number | null;
}

export interface PersonSpend {
  person_id: number | null;
  name: string;
  spending: Cents;
  entries: number;
}

export interface CategorySpend {
  account_id: number;
  name: string;
  flex: Flex | null;
  spending: Cents;
}

export interface MonthBudget {
  month: string;
  from: ISODate;
  to: ISODate;
  day_of_month: number;
  days_in_month: number;
  is_current: boolean;
  plan_from: string | null;
  available: Cents | null;
  income: Cents;
  spending: Cents;
  overall: BudgetStatus | null;
  lines: BudgetLineView[];
  planned_for_people: Cents;
  planned_for_categories: Cents;
  bills_paid: Cents;
  bills_due: Cents;
  people: PersonSpend[];
  categories: CategorySpend[];
}

// ------------------------------------------------------------- recurring bills

export type Cadence = { every: "month"; day: number } | { every: "months"; months: number; day: number } | { every: "week" };

export interface NewRecurringBill {
  name: string;
  /** 0 when it varies. */
  amount: Cents;
  account_id: number;
  paid_from_id: number | null;
  person_id: number | null;
  cadence: Cadence;
  start_date: ISODate;
  auto_pay: boolean;
  match_text: string | null;
}

export interface RecurringBill extends NewRecurringBill {
  id: number;
  active: boolean;
  next_due: ISODate | null;
}

export type DueState = "paid" | "skipped" | "overdue" | "processing" | "due_today" | "upcoming";

export interface DueDate {
  bill_id: number;
  name: string;
  amount: Cents;
  account_id: number;
  person_id: number | null;
  auto_pay: boolean;
  due_date: ISODate;
  state: DueState;
  payment: { entry_id: number; date: ISODate; amount: Cents; manual: boolean } | null;
}

export interface BillSuggestion {
  name: string;
  account_id: number;
  amount: Cents;
  day: number;
  match_text: string;
  months_seen: number;
  paid_from_id: number | null;
}

// ------------------------------------------------------------- goals & insights

export interface NewGoal {
  name: string;
  target: Cents;
  target_date: ISODate | null;
  account_id: number;
  starting: Cents;
  note: string;
}

export interface Goal extends NewGoal {
  id: number;
  achieved_at: string | null;
  archived: boolean;
  saved: Cents;
  contributions: number;
  remaining: Cents;
  permille: number;
  monthly_rate: Cents;
  projected: ISODate | null;
  months_remaining: number | null;
  on_track: boolean | null;
  required_monthly: Cents | null;
}

export interface HouseholdAlert {
  tone: "info" | "caution" | "alarm";
  title: string;
  body: string;
  target: BudgetTarget | null;
  bill_id: number | null;
}

export interface FlexTier {
  flex: Flex | null;
  spending: Cents;
  usual: Cents;
}

export interface Opportunities {
  month: string;
  income: Cents;
  spending: Cents;
  gap: Cents;
  tiers: FlexTier[];
  categories: { account_id: number; name: string; flex: Flex | null; spending: Cents; usual: Cents }[];
  lifestyle_fifth: Cents;
}

export type ScoreBand = "poor" | "fair" | "good" | "very_good" | "excellent";

export interface BudgetScore {
  score: number;
  band: ScoreBand;
  factors: { key: string; label: string; weight: number; fraction: number; points: number; summary: string }[];
}

export interface Win {
  kind: string;
  title: string;
  body: string;
}
