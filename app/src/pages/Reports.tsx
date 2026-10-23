import { keepPreviousData } from "@tanstack/react-query";
import { ArrowDownLeft, ArrowUpRight, BookOpenCheck, ChevronRight, Clock, Hourglass, PiggyBank, Printer, Scale, TrendingUp, Wallet } from "lucide-react";
import type { ReactNode } from "react";
import { Link, useNavigate, useParams, useSearchParams } from "react-router-dom";
import { useApi } from "../api";
import { SCOPE_LABEL, useSides, type BookSides } from "../book";
import { Card, EmptyState, ErrorBanner, Loading, Money, PageHeader } from "../components/ui";
import { date, iso, money, todayISO } from "../format";
import type { AgingBuckets, AgingReport, BalanceSheet, Cents, ISODate, ProfitAndLoss, ReportLine, Scope, Settings, TrialBalance } from "../types";
import "./Reports.css";

// ------------------------------------------------------------- catalog

type Slug = "pnl" | "balance-sheet" | "ar-aging" | "ap-aging" | "trial-balance";

interface ReportDef {
  slug: Slug;
  title: string;
  question: string;
  icon: ReactNode;
  tone: string;
  range: boolean;
  /** Business-only reports (customers and vendors on account). */
  business?: boolean;
}

/** Household books ask the same questions in household words. */
const HOUSEHOLD_WORDS: Record<string, { title: string; question: string }> = {
  pnl: { title: "Income & spending", question: "What came in and where it went, for a period." },
  "balance-sheet": { title: "Net worth", question: "What you have and what you owe, on a given day." },
};

/**
 * A report's title and question for the side being shown. `scope` is the side
 * picked in a two-sided book; undefined means a one-sided book, whose only
 * side decides.
 */
function words(def: ReportDef, scope: Scope | undefined, sides: BookSides) {
  const household = scope === "household" || (!scope && !sides.business);
  return household ? (HOUSEHOLD_WORDS[def.slug] ?? def) : def;
}

const REPORTS: ReportDef[] = [
  { slug: "pnl", title: "Profit & loss", question: "Did you make money? Income minus expenses for a period.", icon: <TrendingUp size={20} />, tone: "pos", range: true },
  { slug: "balance-sheet", title: "Balance sheet", question: "What you own and what you owe, on a given day.", icon: <Scale size={20} />, tone: "", range: false },
  { slug: "ar-aging", title: "Who owes you", question: "Unpaid customer invoices, grouped by how late they are (A/R aging).", icon: <ArrowDownLeft size={20} />, tone: "violet", range: false, business: true },
  { slug: "ap-aging", title: "Who you owe", question: "Unpaid bills to vendors, grouped by how late they are (A/P aging).", icon: <ArrowUpRight size={20} />, tone: "warn", range: false, business: true },
  { slug: "trial-balance", title: "Trial balance", question: "Every account's debit or credit balance — for your accountant.", icon: <BookOpenCheck size={20} />, tone: "", range: false },
];

export default function Reports() {
  const { report } = useParams();
  const def = REPORTS.find((r) => r.slug === report);
  if (!def) return <Gallery />;
  return <ReportView key={def.slug} def={def} />;
}

function Gallery() {
  const sides = useSides();
  const reports = REPORTS.filter((r) => sides.business || !r.business);
  return (
    <div className="page">
      <PageHeader
        title="Reports"
        subtitle={`Clear answers about your ${sides.business ? "business" : "money"}. Every number links back to the transactions behind it.`}
      />
      <div className="report-gallery">
        {reports.map((r) => {
          // The gallery speaks business whenever the book has a business side.
          const w = words(r, sides.business ? undefined : "household", sides);
          return (
            <Link key={r.slug} to={`/reports/${r.slug}`} className="card report-card">
              <span className={`icon-bubble ${r.tone}`}>{r.icon}</span>
              <div className="grow">
                <div className="report-card-title">{w.title}</div>
                <div className="muted small" style={{ marginTop: 3 }}>
                  {w.question}
                </div>
              </div>
              <ChevronRight size={18} className="report-card-go" />
            </Link>
          );
        })}
      </div>
    </div>
  );
}

// ------------------------------------------------------------- dates

function parse(d: ISODate): Date {
  const [y, m, day] = d.split("-").map(Number);
  return new Date(y, m - 1, day);
}

/** First day of the fiscal year containing `d`. */
function fiscalYearStart(d: ISODate, startMonth: number): ISODate {
  const x = parse(d);
  const y = x.getMonth() + 1 >= startMonth ? x.getFullYear() : x.getFullYear() - 1;
  return iso(new Date(y, startMonth - 1, 1));
}

type Preset = "this_month" | "last_month" | "this_quarter" | "ytd" | "last_year";

function presetRange(p: Preset, today: ISODate, fsm: number): [ISODate, ISODate] {
  const t = parse(today);
  const y = t.getFullYear();
  const m = t.getMonth();
  switch (p) {
    case "this_month":
      return [iso(new Date(y, m, 1)), iso(new Date(y, m + 1, 0))];
    case "last_month":
      return [iso(new Date(y, m - 1, 1)), iso(new Date(y, m, 0))];
    case "this_quarter": {
      const q = Math.floor(m / 3) * 3;
      return [iso(new Date(y, q, 1)), iso(new Date(y, q + 3, 0))];
    }
    case "ytd":
      return [fiscalYearStart(today, fsm), today];
    case "last_year": {
      const start = parse(fiscalYearStart(today, fsm));
      return [iso(new Date(start.getFullYear() - 1, start.getMonth(), 1)), iso(new Date(start.getFullYear(), start.getMonth(), 0))];
    }
  }
}

const PRESETS: { value: Preset; label: string }[] = [
  { value: "this_month", label: "This month" },
  { value: "last_month", label: "Last month" },
  { value: "this_quarter", label: "This quarter" },
  { value: "ytd", label: "Year to date" },
  { value: "last_year", label: "Last year" },
];

// ------------------------------------------------------------- report shell

function ReportView({ def }: { def: ReportDef }) {
  const today = todayISO();
  const [search, setSearch] = useSearchParams();
  const settings = useApi<Settings>("settings.get");
  const fsm = settings.data?.fiscal_year_start_month ?? 1;
  const company = settings.data?.prefs["company.name"] ?? "";

  const [defFrom, defTo] = presetRange("ytd", today, fsm);
  const from = search.get("from") ?? defFrom;
  const to = search.get("to") ?? defTo;
  const asOf = search.get("as_of") ?? today;
  const sides = useSides();
  // A two-sided book reports one side at a time, picked by ?side= in the URL;
  // aging reports are business-only. A one-sided book sends no scope at all.
  const scope: Scope | undefined = sides.both ? (def.business ? "business" : ((search.get("side") as Scope | null) ?? sides.main)) : undefined;
  const w = words(def, scope, sides);

  const set = (patch: Record<string, string>) => {
    const next = new URLSearchParams(search);
    for (const [k, v] of Object.entries(patch)) if (v) next.set(k, v);
    setSearch(next, { replace: true });
  };

  const activePreset = PRESETS.find((p) => {
    const [a, b] = presetRange(p.value, today, fsm);
    return a === from && b === to;
  })?.value;

  const period = def.range ? `${date(from)} – ${date(to)}` : `As of ${date(asOf)}`;
  const generated = new Date().toLocaleString("en-US", { dateStyle: "medium", timeStyle: "short" });

  const asOfQuick: { label: string; value: ISODate }[] = [
    { label: "Today", value: today },
    { label: "End of last month", value: iso(new Date(parse(today).getFullYear(), parse(today).getMonth(), 0)) },
    { label: "End of last year", value: addDaysLocal(fiscalYearStart(today, fsm), -1) },
  ];

  return (
    <div className="page report-page">
      <PageHeader
        back={{ to: "/reports", label: "Reports" }}
        title={w.title}
        subtitle={w.question}
        actions={
          <button className="btn btn-secondary" onClick={() => window.print()}>
            <Printer size={16} /> Print / PDF
          </button>
        }
      />

      <div className="print-only print-head">
        {company && <div className="company">{company}</div>}
        <div className="title">{scope ? `${w.title} · ${SCOPE_LABEL[scope]}` : w.title}</div>
        <div className="meta">
          {period} · Generated {generated}
        </div>
      </div>

      <div className="card report-toolbar no-print">
        {sides.both && !def.business && (
          <div className="tabs" role="tablist">
            {sides.sides.map((s) => (
              <button key={s} role="tab" className={`tab ${scope === s ? "active" : ""}`} onClick={() => set({ side: s })}>
                {SCOPE_LABEL[s]}
              </button>
            ))}
          </div>
        )}
        {def.range ? (
          <>
            <div className="tabs" role="tablist" style={{ flexWrap: "wrap" }}>
              {PRESETS.map((p) => (
                <button
                  key={p.value}
                  role="tab"
                  className={`tab ${activePreset === p.value ? "active" : ""}`}
                  onClick={() => {
                    const [a, b] = presetRange(p.value, today, fsm);
                    set({ from: a, to: b });
                  }}
                >
                  {p.label}
                </button>
              ))}
              <span className={`tab ${activePreset ? "" : "active"}`} style={{ cursor: "default" }}>
                Custom
              </span>
            </div>
            <div className="row" style={{ marginLeft: "auto" }}>
              <span className="report-toolbar-label">From</span>
              <input type="date" className="input" value={from} max={to} onChange={(e) => set({ from: e.target.value })} aria-label="From" />
              <span className="report-toolbar-label">to</span>
              <input type="date" className="input" value={to} min={from} onChange={(e) => set({ to: e.target.value })} aria-label="To" />
            </div>
          </>
        ) : (
          <>
            <span className="report-toolbar-label">As of</span>
            <input type="date" className="input" value={asOf} onChange={(e) => set({ as_of: e.target.value })} aria-label="As of" />
            <div className="row" style={{ gap: 6 }}>
              {asOfQuick.map((q) => (
                <button key={q.label} className={`btn btn-sm ${asOf === q.value ? "btn-dark" : "btn-ghost"}`} onClick={() => set({ as_of: q.value })}>
                  {q.label}
                </button>
              ))}
            </div>
          </>
        )}
      </div>

      {def.slug === "pnl" && <PnL from={from} to={to} scope={scope} />}
      {def.slug === "balance-sheet" && <BalanceSheetReport asOf={asOf} scope={scope} />}
      {def.slug === "trial-balance" && <TrialBalanceReport asOf={asOf} scope={scope} />}
      {def.slug === "ar-aging" && <Aging asOf={asOf} kind="ar" />}
      {def.slug === "ap-aging" && <Aging asOf={asOf} kind="ap" />}
    </div>
  );
}

function addDaysLocal(d: ISODate, n: number): ISODate {
  const x = parse(d);
  x.setDate(x.getDate() + n);
  return iso(x);
}

function useReport<T>(method: string, params: unknown) {
  return useApi<T>(method, params, { placeholderData: keepPreviousData });
}

function SummaryStat(props: { icon: ReactNode; tone: string; label: string; cents: Cents; foot?: ReactNode; big?: boolean; color?: boolean }) {
  const cls = props.color ? (props.cents < 0 ? "neg" : props.cents > 0 ? "pos" : "") : "";
  return (
    <div className="card card-pad">
      <div className="stat-label">
        <span className={`icon-bubble ${props.tone}`}>{props.icon}</span>
        {props.label}
      </div>
      <div className={`stat-value num ${cls}`} style={props.big ? { fontSize: 34 } : undefined}>
        {money(props.cents)}
      </div>
      {props.foot && <div className="stat-foot">{props.foot}</div>}
    </div>
  );
}

function BalancedPill({ ok }: { ok: boolean }) {
  return ok ? <span className="pill pill-pos">Balanced ✓</span> : <span className="pill pill-neg">Out of balance</span>;
}

// ------------------------------------------------------------- statement rows

function Section({ label, colSpan = 2 }: { label: string; colSpan?: number }) {
  return (
    <tr className="section">
      <td colSpan={colSpan}>{label}</td>
    </tr>
  );
}

function Lines({ lines, base, empty }: { lines: ReportLine[]; base?: Cents; empty: string }) {
  const navigate = useNavigate();
  if (lines.length === 0)
    return (
      <tr className="empty-line">
        <td colSpan={base !== undefined ? 3 : 2}>{empty}</td>
      </tr>
    );
  return (
    <>
      {lines.map((l) => (
        <tr key={l.account_id} className="line clickable" onClick={() => navigate(`/accounts/${l.account_id}`)} title="See transactions">
          <td>
            {l.code && <span className="code">{l.code}</span>}
            <Link to={`/accounts/${l.account_id}`} className="row-link line-name">
              {l.name}
            </Link>
          </td>
          {base !== undefined && <td className="pct">{base > 0 ? `${Math.round((l.amount / base) * 1000) / 10}%` : ""}</td>}
          <td className="amt">
            <Money cents={l.amount} tone="neg-only" />
          </td>
        </tr>
      ))}
    </>
  );
}

function Total(props: { label: ReactNode; cents: Cents; kind?: "total" | "subtotal" | "grand"; pct?: boolean; tone?: boolean }) {
  const kind = props.kind ?? "total";
  return (
    <tr className={kind}>
      <td>{props.label}</td>
      {props.pct && <td className="pct" />}
      <td className="amt">
        <Money cents={props.cents} tone={props.tone ? "signed" : "neg-only"} />
      </td>
    </tr>
  );
}

// ------------------------------------------------------------- P&L

function PnL({ from, to, scope }: { from: ISODate; to: ISODate; scope?: Scope }) {
  const { data, error, isLoading } = useReport<ProfitAndLoss>("reports.pnl", { from, to, scope });
  const sides = useSides();
  // A household doesn't make a profit; what's left is just left over.
  const household = scope === "household" || (!scope && !sides.business);
  if (isLoading) return <Loading />;
  if (error || !data) return <ErrorBanner error={error} />;

  const hasCogs = data.cost_of_goods_sold.length > 0 || data.total_cost_of_goods_sold !== 0;
  const margin = data.total_income > 0 ? Math.round((data.net_income / data.total_income) * 100) : null;
  const expenses = [...data.expenses].sort((a, b) => b.amount - a.amount);
  const maxExpense = Math.max(1, ...expenses.map((e) => Math.abs(e.amount)));
  const base = data.total_income;
  const isEmpty = data.income.length === 0 && data.expenses.length === 0 && data.cost_of_goods_sold.length === 0;

  return (
    <>
      <div className="report-summary">
        <SummaryStat
          icon={<PiggyBank size={18} />}
          tone={data.net_income < 0 ? "neg" : "pos"}
          label={household ? (data.net_income < 0 ? "Overspent" : "Left over") : data.net_income < 0 ? "Net loss" : "Net profit"}
          cents={data.net_income}
          big
          color
          foot={margin !== null ? `${margin}% of income ${household ? "left over" : "kept as profit"}` : "No income in this period"}
        />
        <SummaryStat icon={<ArrowDownLeft size={18} />} tone="pos" label="Income" cents={data.total_income} foot={`${data.income.length} income ${data.income.length === 1 ? "account" : "accounts"}`} />
        <SummaryStat
          icon={<ArrowUpRight size={18} />}
          tone="neg"
          label="Expenses"
          cents={data.total_expenses + data.total_cost_of_goods_sold}
          foot={hasCogs ? `Includes ${money(data.total_cost_of_goods_sold)} cost of goods sold` : `${data.expenses.length} spending categories`}
        />
      </div>

      {isEmpty ? (
        <Card>
          <EmptyState icon={<TrendingUp size={22} />} title="Nothing in this period" body="No income or expenses were recorded between these dates. Try a wider range." />
        </Card>
      ) : (
        <div className="report-two-col">
          <Card pad={false}>
            <table className="statement">
              <tbody>
                <Section label="Income" colSpan={3} />
                <Lines lines={data.income} base={base} empty="No income" />
                <Total label="Total income" cents={data.total_income} pct />

                {hasCogs && (
                  <>
                    <Section label="Cost of goods sold" colSpan={3} />
                    <Lines lines={data.cost_of_goods_sold} base={base} empty="None" />
                    <Total label="Total cost of goods sold" cents={data.total_cost_of_goods_sold} pct />
                    <Total label="Gross profit" cents={data.gross_profit} kind="subtotal" pct />
                  </>
                )}

                <Section label="Expenses" colSpan={3} />
                <Lines lines={data.expenses} base={base} empty="No expenses" />
                <Total label="Total expenses" cents={data.total_expenses} pct />

                <Total label={data.net_income < 0 ? "Net loss" : "Net income"} cents={data.net_income} kind="grand" pct tone />
              </tbody>
            </table>
          </Card>

          <Card title="Where the money went" className="no-print">
            {expenses.length === 0 ? (
              <EmptyState icon={<Wallet size={22} />} title="No spending" body="Categorized expenses show up here." />
            ) : (
              <div className="stack" style={{ gap: 14 }}>
                {expenses.map((e) => (
                  <Link key={e.account_id} to={`/accounts/${e.account_id}`} style={{ color: "inherit" }}>
                    <div className="row-between" style={{ marginBottom: 6 }}>
                      <span className="bold truncate">{e.name}</span>
                      <span className="row" style={{ gap: 8 }}>
                        {data.total_expenses > 0 && <span className="subtle small num">{Math.round((e.amount / data.total_expenses) * 100)}%</span>}
                        <Money cents={e.amount} className="bold" tone="neg-only" />
                      </span>
                    </div>
                    <div className="progress">
                      <div style={{ width: `${(Math.abs(e.amount) / maxExpense) * 100}%` }} />
                    </div>
                  </Link>
                ))}
              </div>
            )}
          </Card>
        </div>
      )}
    </>
  );
}

// ------------------------------------------------------------- balance sheet

function BalanceSheetReport({ asOf, scope }: { asOf: ISODate; scope?: Scope }) {
  const { data, error, isLoading } = useReport<BalanceSheet>("reports.balance_sheet", { as_of: asOf, scope });
  if (isLoading) return <Loading />;
  if (error || !data) return <ErrorBanner error={error} />;

  const liabPlusEquity = data.total_liabilities + data.total_equity;
  const balanced = data.total_assets === liabPlusEquity;

  return (
    <>
      <div className="report-summary">
        <SummaryStat icon={<PiggyBank size={18} />} tone={data.total_equity < 0 ? "neg" : "pos"} label="Net worth (equity)" cents={data.total_equity} big color foot="What you own minus what you owe" />
        <SummaryStat icon={<Wallet size={18} />} tone="" label="What you own" cents={data.total_assets} foot="Cash, money owed to you, and other assets" />
        <SummaryStat icon={<ArrowUpRight size={18} />} tone="warn" label="What you owe" cents={data.total_liabilities} foot="Cards, unpaid bills, and loans" />
      </div>

      <Card pad={false}>
        <div className="row-between" style={{ padding: "16px 22px 0" }}>
          <h3 className="card-title">Statement</h3>
          <BalancedPill ok={balanced} />
        </div>
        <table className="statement">
          <tbody>
            <Section label="Assets — what you own" />
            <Lines lines={data.assets} empty="No assets" />
            <Total label="Total assets" cents={data.total_assets} />

            <Section label="Liabilities — what you owe" />
            <Lines lines={data.liabilities} empty="No liabilities" />
            <Total label="Total liabilities" cents={data.total_liabilities} />

            <Section label="Equity — what's yours" />
            {data.equity.length > 0 && <Lines lines={data.equity} empty="" />}
            <tr className="line">
              <td>Retained earnings</td>
              <td className="amt">
                <Money cents={data.retained_earnings} tone="neg-only" />
              </td>
            </tr>
            <tr className="line">
              <td>Net income (this year)</td>
              <td className="amt">
                <Money cents={data.net_income} tone="neg-only" />
              </td>
            </tr>
            <Total label="Total equity" cents={data.total_equity} />

            <tr className="grand">
              <td>
                <span className="row" style={{ gap: 10 }}>
                  Liabilities + Equity <span className="no-print"><BalancedPill ok={balanced} /></span>
                </span>
              </td>
              <td className="amt">
                <Money cents={liabPlusEquity} tone="neg-only" />
              </td>
            </tr>
          </tbody>
        </table>
      </Card>
    </>
  );
}

// ------------------------------------------------------------- trial balance

function TrialBalanceReport({ asOf, scope }: { asOf: ISODate; scope?: Scope }) {
  const navigate = useNavigate();
  const { data, error, isLoading } = useReport<TrialBalance>("reports.trial_balance", { as_of: asOf, scope });
  if (isLoading) return <Loading />;
  if (error || !data) return <ErrorBanner error={error} />;
  const balanced = data.total_debits === data.total_credits;

  return (
    <Card title="Trial balance" action={<BalancedPill ok={balanced} />} pad={false}>
      {data.rows.length === 0 ? (
        <EmptyState icon={<BookOpenCheck size={22} />} title="No activity yet" body="Accounts with a balance on this date will be listed here." />
      ) : (
        <table className="table" style={{ marginTop: 10 }}>
          <thead>
            <tr>
              <th style={{ width: 90 }}>Code</th>
              <th>Account</th>
              <th className="right">Debit</th>
              <th className="right">Credit</th>
            </tr>
          </thead>
          <tbody>
            {data.rows.map((r) => (
              <tr key={r.account_id} className="clickable" onClick={() => navigate(`/accounts/${r.account_id}`)}>
                <td className="subtle num">{r.code ?? "—"}</td>
                <td className="bold">
                  <Link to={`/accounts/${r.account_id}`} className="row-link">
                    {r.name}
                  </Link>
                </td>
                <td className="right num">{r.debit ? money(r.debit) : ""}</td>
                <td className="right num">{r.credit ? money(r.credit) : ""}</td>
              </tr>
            ))}
          </tbody>
          <tfoot>
            <tr>
              <td />
              <td>Totals</td>
              <td className="right num">{money(data.total_debits)}</td>
              <td className="right num">{money(data.total_credits)}</td>
            </tr>
          </tfoot>
        </table>
      )}
    </Card>
  );
}

// ------------------------------------------------------------- aging

const BUCKETS: { key: keyof AgingBuckets; label: string; cls: string; color: string }[] = [
  { key: "current", label: "Current", cls: "", color: "var(--brand)" },
  { key: "days_1_30", label: "1–30 days", cls: "aging-1", color: "color-mix(in srgb, var(--warn) 55%, var(--surface))" },
  { key: "days_31_60", label: "31–60 days", cls: "aging-2", color: "var(--warn)" },
  { key: "days_61_90", label: "61–90 days", cls: "aging-3", color: "color-mix(in srgb, var(--warn) 50%, var(--neg))" },
  { key: "over_90", label: "90+ days", cls: "aging-4", color: "var(--neg)" },
];

function bucketTotal(b: AgingBuckets): Cents {
  return b.current + b.days_1_30 + b.days_31_60 + b.days_61_90 + b.over_90;
}

function Aging({ asOf, kind }: { asOf: ISODate; kind: "ar" | "ap" }) {
  const { data, error, isLoading } = useReport<AgingReport>(kind === "ar" ? "reports.ar_aging" : "reports.ap_aging", { as_of: asOf });
  if (isLoading) return <Loading />;
  if (error || !data) return <ErrorBanner error={error} />;

  const total = bucketTotal(data.totals);
  const overdue = total - data.totals.current;
  const who = kind === "ar" ? "customer" : "vendor";

  if (data.rows.length === 0)
    return (
      <Card>
        <EmptyState
          icon={kind === "ar" ? <ArrowDownLeft size={22} /> : <ArrowUpRight size={22} />}
          title={kind === "ar" ? "Nobody owes you anything" : "You're all paid up"}
          body={kind === "ar" ? "Every invoice was paid as of this date." : "No unpaid bills as of this date."}
          action={
            <Link to={kind === "ar" ? "/invoices" : "/bills"} className="btn btn-secondary no-print">
              {kind === "ar" ? "See invoices" : "See bills"}
            </Link>
          }
        />
      </Card>
    );

  return (
    <div className="stack" style={{ gap: 20 }}>
      <div className="report-summary">
        <SummaryStat
          icon={kind === "ar" ? <ArrowDownLeft size={18} /> : <ArrowUpRight size={18} />}
          tone={kind === "ar" ? "violet" : "warn"}
          label={kind === "ar" ? "Total owed to you" : "Total you owe"}
          cents={total}
          big
          foot={`${data.rows.length} ${who}${data.rows.length === 1 ? "" : "s"} with an open balance`}
        />
        <SummaryStat icon={<Clock size={18} />} tone="pos" label="Not yet due" cents={data.totals.current} foot={total ? `${Math.round((data.totals.current / total) * 100)}% of the total` : undefined} />
        <SummaryStat
          icon={<Hourglass size={18} />}
          tone={overdue > 0 ? "neg" : ""}
          label="Past due"
          cents={overdue}
          foot={data.totals.over_90 > 0 ? `${money(data.totals.over_90)} is more than 90 days late` : overdue > 0 ? "Worth a friendly nudge" : "Nothing late"}
        />
      </div>

      <Card title="By age">
        <div className="aging-bar" role="img" aria-label="Balances by age">
          {BUCKETS.filter((b) => data.totals[b.key] > 0).map((b) => (
            <div key={b.key} title={`${b.label}: ${money(data.totals[b.key])}`} style={{ width: `${(data.totals[b.key] / Math.max(1, total)) * 100}%`, background: b.color }} />
          ))}
        </div>
        <div className="aging-legend small">
          {BUCKETS.map((b) => (
            <span key={b.key} className="row" style={{ gap: 6 }}>
              <i className="swatch" style={{ background: b.color }} />
              <span className="muted bold">{b.label}</span>
              <span className={`num bold ${data.totals[b.key] ? b.cls : "zero"}`}>{money(data.totals[b.key])}</span>
            </span>
          ))}
        </div>
      </Card>

      <Card pad={false}>
        <table className="table">
          <thead>
            <tr>
              <th>{kind === "ar" ? "Customer" : "Vendor"}</th>
              {BUCKETS.map((b) => (
                <th key={b.key} className="right">
                  {b.label.replace(" days", "")}
                </th>
              ))}
              <th className="right">Total</th>
            </tr>
          </thead>
          <tbody>
            {data.rows.map((r) => (
              <tr key={r.contact_id}>
                <td className="bold">{r.name}</td>
                {BUCKETS.map((b) => (
                  <BucketCell key={b.key} cents={r.buckets[b.key]} cls={b.cls} />
                ))}
                <td className="right num bold">{money(bucketTotal(r.buckets))}</td>
              </tr>
            ))}
          </tbody>
          <tfoot>
            <tr>
              <td>Total</td>
              {BUCKETS.map((b) => (
                <BucketCell key={b.key} cents={data.totals[b.key]} cls={b.cls} />
              ))}
              <td className="right num">{money(total)}</td>
            </tr>
          </tfoot>
        </table>
      </Card>
    </div>
  );
}

function BucketCell({ cents, cls }: { cents: Cents; cls: string }) {
  return <td className={`right num ${cents ? cls : "zero"}`}>{cents ? money(cents) : "—"}</td>;
}
