// Home for a business book: cash on hand, card balances, money in and out,
// where money went, and the things waiting for the owner.

import {
  AlertCircle,
  Banknote,
  ArrowDownLeft,
  ArrowUpRight,
  CalendarClock,
  CreditCard,
  FileText,
  Inbox,
  Landmark,
  Plus,
  Receipt,
  RefreshCw,
  Upload,
} from "lucide-react";
import { lazy, Suspense, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { useAction, useApi } from "../api";
import { BigMoney, Card, EmptyState, ErrorBanner, Loading, Money, useToast } from "../components/ui";
import LogMoneyModal from "../flows/LogMoneyModal";
import { money, plural } from "../format";
import type { Dashboard as DashboardData, SyncReport } from "../types";

const CashFlowChart = lazy(() => import("../components/CashFlowChart"));

function greeting() {
  const h = new Date().getHours();
  return h < 12 ? "Good morning" : h < 18 ? "Good afternoon" : "Good evening";
}

function Change({ now, before, invert }: { now: number; before: number; invert?: boolean }) {
  if (!before) return <span className="stat-foot">No data last month</span>;
  const pct = Math.round(((now - before) / Math.abs(before)) * 100);
  const good = invert ? pct <= 0 : pct >= 0;
  return (
    <span className={`stat-foot ${good ? "pos" : "neg"}`} style={{ fontWeight: 700 }}>
      {pct >= 0 ? "▲" : "▼"} {Math.abs(pct)}% <span className="subtle" style={{ fontWeight: 500 }}>vs last month</span>
    </span>
  );
}

export default function Dashboard() {
  const navigate = useNavigate();
  const toast = useToast();
  const { data, error, isLoading } = useApi<DashboardData>("dashboard");
  const status = useApi<{ company_name: string | null }>("book.status");
  const sync = useAction<SyncReport[]>("bank.sync");
  const [logging, setLogging] = useState(false);

  if (isLoading) return <Loading />;
  if (error || !data) return <div className="page"><ErrorBanner error={error} /></div>;

  const linked = data.accounts.some((a) => a.linked);
  const net = data.cash - data.credit_owed;
  const maxExpense = Math.max(1, ...data.top_expenses.map((c) => c.amount));

  const doSync = async () => {
    try {
      const reports = await sync.mutateAsync({});
      const added = reports.reduce((n, r) => n + r.imported.added, 0);
      const failed = reports.filter((r) => !r.ok);
      if (failed.length) toast.error(`${failed[0].institution}: ${failed[0].error}`);
      else toast({ text: added ? `${plural(added, "new transaction")} to review` : "You're up to date" });
    } catch (e) {
      toast.error(e);
    }
  };

  return (
    <div className="page">
      <div className="page-header">
        <div>
          <div className="muted" style={{ fontWeight: 600 }}>{greeting()} 👋</div>
          <h1 className="page-title">{status.data?.company_name ?? "Your business"}</h1>
        </div>
        <div className="row">
          {linked && (
            <button className="btn btn-secondary" onClick={doSync} disabled={sync.isPending}>
              <RefreshCw size={16} className={sync.isPending ? "spin" : ""} /> {sync.isPending ? "Syncing…" : "Sync banks"}
            </button>
          )}
          <button className="btn btn-secondary" onClick={() => setLogging(true)}>
            <Banknote size={16} /> Log cash
          </button>
          <Link to="/invoices/new" className="btn btn-primary">
            <Plus size={16} /> New invoice
          </Link>
        </div>
      </div>

      <div className="stack" style={{ gap: 20 }}>
        {/* Hero: money at a glance */}
        <div className="hero">
          <div className="row-between" style={{ alignItems: "flex-start" }}>
            <div>
              <div className="hero-label">Cash on hand</div>
              <div className="hero-amount">
                <BigMoney cents={data.cash} />
              </div>
              <div className="row" style={{ marginTop: 10, gap: 18, color: "var(--hero-ink-2)", fontWeight: 600 }}>
                <span>
                  After cards <span style={{ color: "#fff" }}>{money(net)}</span>
                </span>
                {data.credit_owed > 0 && (
                  <span>
                    Card balances <span style={{ color: "#fff" }}>{money(data.credit_owed)}</span>
                  </span>
                )}
              </div>
            </div>
            <div className="row" style={{ position: "relative", zIndex: 1 }}>
              <Link to="/banking" className="btn btn-on-hero btn-sm">
                <Landmark size={14} /> {linked ? "Manage banks" : "Connect a bank"}
              </Link>
            </div>
          </div>

          {data.accounts.length > 0 ? (
            <div style={{ display: "flex", gap: 12, marginTop: 26, overflowX: "auto", position: "relative", zIndex: 1 }}>
              {data.accounts.map((a) => (
                <button
                  key={a.id}
                  onClick={() => navigate(`/accounts/${a.id}`)}
                  style={{
                    flex: "0 0 auto",
                    minWidth: 190,
                    textAlign: "left",
                    background: "rgba(255,255,255,.1)",
                    border: "1px solid rgba(255,255,255,.14)",
                    borderRadius: 18,
                    padding: "14px 16px",
                    color: "#fff",
                    cursor: "pointer",
                  }}
                >
                  <div className="row" style={{ color: "var(--hero-ink-2)", fontSize: 12.5, fontWeight: 600 }}>
                    {a.subtype === "credit_card" ? <CreditCard size={14} /> : <Landmark size={14} />}
                    <span className="truncate">{a.name}</span>
                    {a.mask && <span>···{a.mask}</span>}
                  </div>
                  <div className="num" style={{ fontSize: 20, fontWeight: 800, letterSpacing: "-.03em", marginTop: 6 }}>
                    {money(a.balance)}
                  </div>
                  {a.bank_balance != null && a.bank_balance !== a.balance && (
                    <div style={{ fontSize: 11.5, color: "var(--hero-ink-2)", marginTop: 2 }}>Bank says {money(a.bank_balance)}</div>
                  )}
                </button>
              ))}
            </div>
          ) : (
            <div className="row" style={{ marginTop: 22, gap: 10, position: "relative", zIndex: 1 }}>
              <Link to="/banking" className="btn btn-on-hero">
                <Landmark size={16} /> Connect your bank
              </Link>
              <Link to="/accounts" className="btn btn-on-hero">
                <Plus size={16} /> Add an account manually
              </Link>
            </div>
          )}
        </div>

        {/* To-do strip */}
        {(data.inbox_count > 0 || data.overdue_invoices.count > 0 || data.bills_due_soon.count > 0) && (
          <div className="grid-3">
            {data.inbox_count > 0 && (
              <TodoCard to="/review" icon={<Inbox size={18} />} tone="" title={`${plural(data.inbox_count, "transaction")} to review`} body="Categorize them to keep your books current" />
            )}
            {data.overdue_invoices.count > 0 && (
              <TodoCard
                to="/invoices"
                icon={<AlertCircle size={18} />}
                tone="neg"
                title={`${money(data.overdue_invoices.amount)} overdue`}
                body={`${plural(data.overdue_invoices.count, "invoice")} past due — time for a nudge`}
              />
            )}
            {data.bills_due_soon.count > 0 && (
              <TodoCard
                to="/bills"
                icon={<CalendarClock size={18} />}
                tone="warn"
                title={`${money(data.bills_due_soon.amount)} due this week`}
                body={`${plural(data.bills_due_soon.count, "bill")} to pay soon`}
              />
            )}
          </div>
        )}

        {/* KPI cards */}
        <div className="grid-4">
          <Stat icon={<ArrowDownLeft size={18} />} tone="pos" label="Money in · this month" value={data.this_month.income}>
            <Change now={data.this_month.income} before={data.last_month.income} />
          </Stat>
          <Stat icon={<ArrowUpRight size={18} />} tone="neg" label="Money out · this month" value={data.this_month.expenses}>
            <Change now={data.this_month.expenses} before={data.last_month.expenses} invert />
          </Stat>
          <Stat icon={<FileText size={18} />} tone="violet" label="Owed to you" value={data.receivable} to="/invoices">
            <span className="stat-foot">Unpaid invoices</span>
          </Stat>
          <Stat icon={<Receipt size={18} />} tone="warn" label="You owe" value={data.payable} to="/bills">
            <span className="stat-foot">Unpaid bills</span>
          </Stat>
        </div>

        <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1.7fr) minmax(0, 1fr)", gap: 20 }}>
          <Card
            title="Cash flow"
            action={
              <div className="row small muted" style={{ gap: 14, fontWeight: 600 }}>
                <span className="row" style={{ gap: 6 }}>
                  <i style={{ width: 10, height: 10, borderRadius: 3, background: "var(--brand)", display: "inline-block" }} /> In
                </span>
                <span className="row" style={{ gap: 6 }}>
                  <i style={{ width: 10, height: 10, borderRadius: 3, background: "var(--line-strong)", display: "inline-block" }} /> Out
                </span>
              </div>
            }
          >
            {/* Charts load on their own, so the numbers above never wait for them. */}
            <Suspense fallback={<div style={{ height: 250 }} />}>
              <CashFlowChart data={data.cashflow} />
            </Suspense>
          </Card>

          <Card title="Where money went" action={<span className="subtle small">This month</span>}>
            {data.top_expenses.length === 0 ? (
              <EmptyState icon={<Receipt size={22} />} title="No spending yet" body="Categorized expenses show up here." />
            ) : (
              <div className="stack" style={{ gap: 14 }}>
                {data.top_expenses.map((c) => (
                  <Link key={c.account_id} to={`/accounts/${c.account_id}`} style={{ color: "inherit" }}>
                    <div className="row-between" style={{ marginBottom: 6 }}>
                      <span className="bold truncate">{c.name}</span>
                      <Money cents={c.amount} className="bold" />
                    </div>
                    <div className="progress">
                      <div style={{ width: `${(c.amount / maxExpense) * 100}%` }} />
                    </div>
                  </Link>
                ))}
                <div className="row-between" style={{ marginTop: 4, paddingTop: 12, borderTop: "1px solid var(--line)" }}>
                  <span className="muted bold">Net this month</span>
                  <Money cents={data.this_month.net} tone="signed" className="bold" signed />
                </div>
              </div>
            )}
          </Card>
        </div>

        <div className="grid-4 no-print">
          <QuickAction to="/invoices/new" icon={<FileText size={18} />} label="Send an invoice" />
          <QuickAction to="/bills/new" icon={<Receipt size={18} />} label="Enter a bill" />
          <QuickAction to="/review?import=1" icon={<Upload size={18} />} label="Import a statement" />
          <QuickAction to="/reports" icon={<ArrowUpRight size={18} />} label="See reports" />
        </div>
      </div>
      <LogMoneyModal open={logging} onClose={() => setLogging(false)} scope="business" />
    </div>
  );
}

function Stat(props: { icon: React.ReactNode; tone: string; label: string; value: number; to?: string; children?: React.ReactNode }) {
  const body = (
    <div className="card card-pad" style={{ height: "100%" }}>
      <div className="stat-label">
        <span className={`icon-bubble ${props.tone}`}>{props.icon}</span>
        {props.label}
      </div>
      <div className="stat-value num">{money(props.value)}</div>
      {props.children}
    </div>
  );
  return props.to ? (
    <Link to={props.to} style={{ color: "inherit" }}>
      {body}
    </Link>
  ) : (
    body
  );
}

function TodoCard(props: { to: string; icon: React.ReactNode; tone: string; title: string; body: string }) {
  return (
    <Link to={props.to} className="card card-pad row" style={{ color: "inherit", gap: 14 }}>
      <span className={`icon-bubble ${props.tone}`}>{props.icon}</span>
      <div className="grow">
        <div className="bold">{props.title}</div>
        <div className="muted small">{props.body}</div>
      </div>
      <ArrowUpRight size={16} className="subtle" />
    </Link>
  );
}

function QuickAction(props: { to: string; icon: React.ReactNode; label: string }) {
  return (
    <Link to={props.to} className="card row" style={{ padding: "14px 16px", color: "inherit", gap: 12, fontWeight: 700 }}>
      <span className="icon-bubble">{props.icon}</span>
      {props.label}
    </Link>
  );
}
