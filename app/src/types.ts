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

export interface Account {
  id: number;
  code: string | null;
  name: string;
  subtype: AccountSubtype;
  parent_id: number | null;
  archived: boolean;
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

export interface InboxItem extends BankTxn {
  suggestion: Suggestion | null;
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
  today: ISODate;
}

export interface RecentBook {
  path: string;
  name: string;
  last_opened: string;
}

export interface Settings {
  prefs: Record<string, string>;
  fiscal_year_start_month: number;
  lock_date: ISODate | null;
}
