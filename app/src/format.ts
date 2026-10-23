// Formatting and parsing at the UI edge. The backend only ever sees cents
// and ISO dates.

import type { AccountKind, AccountSubtype, Cadence, Cents, Flex, ISODate, Scope } from "./types";

const usd = new Intl.NumberFormat("en-US", { style: "currency", currency: "USD" });
const usdWhole = new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 0 });

/** 123456 → "$1,234.56"; -500 → "-$5.00". `signed` adds "+" to positives. */
export function money(cents: Cents, opts: { signed?: boolean; whole?: boolean } = {}): string {
  const s = (opts.whole ? usdWhole : usd).format(cents / 100);
  return opts.signed && cents > 0 ? `+${s}` : s;
}

/** For chart axes: "$12.4k", "$1.2M". */
export function moneyCompact(cents: Cents): string {
  const v = cents / 100;
  const abs = Math.abs(v);
  // 999,500 and up rounds to 1000k, so it's already in millions' territory.
  if (abs >= 999_500) return `${v < 0 ? "-" : ""}$${(abs / 1_000_000).toFixed(1)}M`;
  if (abs >= 10_000) return `${v < 0 ? "-" : ""}$${Math.round(abs / 1000)}k`;
  if (abs >= 1_000) return `${v < 0 ? "-" : ""}$${(abs / 1000).toFixed(1)}k`;
  return `${v < 0 ? "-" : ""}$${Math.round(abs)}`;
}

/** Splits for display: "$1,234" + ".56". */
export function moneyParts(cents: Cents): { whole: string; fraction: string } {
  const s = money(cents);
  const i = s.lastIndexOf(".");
  return i < 0 ? { whole: s, fraction: "" } : { whole: s.slice(0, i), fraction: s.slice(i) };
}

/** "1,234.5" / "$12" / "(4.00)" / "-3" → cents, or null if not a number. Rounds to the cent. */
export function parseMoney(input: string): Cents | null {
  let s = input.trim();
  if (!s) return null;
  let neg = false;
  if (s.startsWith("(") && s.endsWith(")")) {
    neg = true;
    s = s.slice(1, -1);
  }
  if (s.startsWith("-")) {
    neg = !neg;
    s = s.slice(1);
  }
  s = s.replace(/[$,\s]/g, "");
  if (!/^\d*\.?\d*$/.test(s) || s === "" || s === ".") return null;
  const [w, f = ""] = s.split(".");
  const cents = Number(w || "0") * 100 + Number((f + "00").slice(0, 2)) + (f.length > 2 && Number(f[2]) >= 5 ? 1 : 0);
  if (!Number.isSafeInteger(cents)) return null;
  return neg ? -cents : cents;
}

/** Cents → editable string without symbols: 123456 → "1234.56". */
export function centsToInput(cents: Cents | null | undefined): string {
  if (cents == null) return "";
  const neg = cents < 0;
  const abs = Math.abs(cents);
  return `${neg ? "-" : ""}${Math.floor(abs / 100)}.${String(abs % 100).padStart(2, "0")}`;
}

/** 1500 → "1.5" */
export function qty(milli: number): string {
  return String(milli / 1000);
}

export function parseQty(input: string): number | null {
  const s = input.trim();
  if (!/^\d*\.?\d{0,3}$/.test(s) || s === "" || s === ".") return null;
  return Math.round(Number(s) * 1000);
}

function toDate(iso: ISODate): Date {
  const [y, m, d] = iso.split("-").map(Number);
  return new Date(y, m - 1, d);
}

/** "2026-03-04" → "Mar 4, 2026" */
export function date(iso: ISODate | null | undefined): string {
  if (!iso) return "";
  return toDate(iso).toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric" });
}

/** "2026-03-04" → "Mar 4" */
export function shortDate(iso: ISODate): string {
  return toDate(iso).toLocaleDateString("en-US", { month: "short", day: "numeric" });
}

/** "2026-03" → "Mar" */
export function monthLabel(ym: string): string {
  return toDate(`${ym}-01`).toLocaleDateString("en-US", { month: "short" });
}

export function iso(d: Date): ISODate {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

export function todayISO(): ISODate {
  return iso(new Date());
}

export function addDays(day: ISODate, n: number): ISODate {
  const d = toDate(day);
  d.setDate(d.getDate() + n);
  return iso(d);
}

export function startOfMonth(day: ISODate): ISODate {
  return `${day.slice(0, 7)}-01`;
}

export function startOfYear(day: ISODate): ISODate {
  return `${day.slice(0, 4)}-01-01`;
}

export function daysBetween(a: ISODate, b: ISODate): number {
  return Math.round((toDate(b).getTime() - toDate(a).getTime()) / 86_400_000);
}

export const SUBTYPE_LABEL: Record<AccountSubtype, string> = {
  bank: "Bank",
  accounts_receivable: "Accounts receivable",
  other_current_asset: "Other current asset",
  fixed_asset: "Fixed asset",
  credit_card: "Credit card",
  accounts_payable: "Accounts payable",
  other_current_liability: "Other current liability",
  long_term_liability: "Long-term liability",
  equity: "Equity",
  retained_earnings: "Retained earnings",
  income: "Income",
  other_income: "Other income",
  cost_of_goods_sold: "Cost of goods sold",
  expense: "Expense",
  other_expense: "Other expense",
};

export const KIND_LABEL: Record<AccountKind, string> = {
  asset: "Assets",
  liability: "Liabilities",
  equity: "Equity",
  income: "Income",
  expense: "Expenses",
};

export const KIND_ORDER: AccountKind[] = ["asset", "liability", "equity", "income", "expense"];

/** A group heading for accounts of one kind; households spend rather than expense. */
export function kindLabel(kind: AccountKind, scope: Scope, withSide = false): string {
  const base = scope === "household" && kind === "expense" ? "Spending" : KIND_LABEL[kind];
  return withSide ? `${scope === "household" ? "Household" : "Business"} · ${base}` : base;
}

/** How far a household category can bend, in plain words. */
export const FLEX_LABEL: Record<Flex, string> = {
  fixed: "Fixed",
  usage: "Moves with use",
  lifestyle: "Your choice",
};

const ORDINAL_SUFFIX: Record<number, string> = { 1: "st", 2: "nd", 3: "rd" };

/** 1 → "1st", 2 → "2nd", 11 → "11th", 23 → "23rd". */
export function ordinal(n: number): string {
  const lastTwo = n % 100;
  // 11th, 12th and 13th break the pattern of their last digit.
  if (lastTwo >= 11 && lastTwo <= 13) return `${n}th`;
  return `${n}${ORDINAL_SUFFIX[n % 10] ?? "th"}`;
}

/** How often a bill comes around, in words: "Monthly on the 3rd", "Yearly on the 15th", "Weekly". */
export function cadenceLabel(c: Cadence): string {
  if (c.every === "week") return "Weekly";
  if (c.every === "months") return c.months === 12 ? `Yearly on the ${ordinal(c.day)}` : `Every ${c.months} months on the ${ordinal(c.day)}`;
  return `Monthly on the ${ordinal(c.day)}`;
}

export function plural(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}
