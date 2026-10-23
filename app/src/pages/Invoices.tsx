import { AlertCircle, CheckCircle2, FilePen, FileText, HandCoins, Plus, Search, Wallet } from "lucide-react";
import { useMemo, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { useApi } from "../api";
import { Avatar, Card, EmptyState, ErrorBanner, Loading, Money, PageHeader, StatusPill, Tabs } from "../components/ui";
import ReceivePaymentModal from "../flows/ReceivePaymentModal";
import { addDays, date, daysBetween, money, plural, todayISO } from "../format";
import type { InvoiceSummary, PaymentSummary } from "../types";
import "./Invoice.css";

type Filter = "all" | "unpaid" | "overdue" | "paid" | "draft";

const UNPAID = new Set(["open", "partial", "overdue"]);

const matches: Record<Filter, (i: InvoiceSummary) => boolean> = {
  all: () => true,
  unpaid: (i) => UNPAID.has(i.status),
  overdue: (i) => i.status === "overdue",
  paid: (i) => i.status === "paid",
  draft: (i) => i.status === "draft",
};

/** "Due in 5 days" / "Due today" / "12 days late" for unpaid invoices. */
function dueHint(inv: InvoiceSummary, today: string): { text: string; cls: string } | null {
  if (!UNPAID.has(inv.status)) return null;
  const d = daysBetween(today, inv.due_date);
  if (d < 0) return { text: `${plural(-d, "day")} late`, cls: "late" };
  if (d === 0) return { text: "Due today", cls: "soon" };
  return { text: `Due in ${plural(d, "day")}`, cls: d <= 7 ? "soon" : "" };
}

export default function Invoices() {
  const navigate = useNavigate();
  const { data, error, isLoading } = useApi<InvoiceSummary[]>("invoices.list");
  const [filter, setFilter] = useState<Filter>("all");
  const [query, setQuery] = useState("");
  const [paying, setPaying] = useState(false);
  const today = todayISO();

  // Keeps its identity while the data is unchanged, so the tables below don't recompute every render.
  const invoices = useMemo(() => data ?? [], [data]);
  const received = useApi<PaymentSummary[]>("payments.list", { kind: "received", from: addDays(today, -30) });

  const stats = useMemo(() => {
    const cutoff = addDays(today, -30);
    const unpaid = invoices.filter((i) => UNPAID.has(i.status));
    const overdue = invoices.filter((i) => i.status === "overdue");
    const paid30 = (received.data ?? []).filter((p) => !p.voided && p.date >= cutoff);
    return {
      unpaid: unpaid.reduce((s, i) => s + i.balance, 0),
      unpaidCount: unpaid.length,
      overdue: overdue.reduce((s, i) => s + i.balance, 0),
      overdueCount: overdue.length,
      paid30: paid30.reduce((s, p) => s + p.amount, 0),
      paid30Count: paid30.length,
      drafts: invoices.filter((i) => i.status === "draft").length,
    };
  }, [invoices, received.data, today]);

  const counts = useMemo(() => {
    const c = {} as Record<Filter, number>;
    (Object.keys(matches) as Filter[]).forEach((f) => (c[f] = invoices.filter(matches[f]).length));
    return c;
  }, [invoices]);

  const rows = useMemo(() => {
    const q = query.trim().toLowerCase();
    return invoices.filter(matches[filter]).filter((i) => !q || i.customer_name.toLowerCase().includes(q) || i.number.toLowerCase().includes(q));
  }, [invoices, filter, query]);

  if (isLoading) return <Loading />;

  const actions = (
    <>
      {invoices.length > 0 && (
        <button className="btn btn-secondary" onClick={() => setPaying(true)}>
          <HandCoins size={16} /> Receive payment
        </button>
      )}
      <Link to="/invoices/new" className="btn btn-primary">
        <Plus size={16} /> New invoice
      </Link>
    </>
  );

  return (
    <div className="page">
      <PageHeader title="Invoices" subtitle="Bill your customers and keep track of who's paid." actions={actions} />
      <ErrorBanner error={error} />

      {!error && invoices.length === 0 ? (
        <Card>
          <EmptyState
            icon={<FileText size={26} />}
            title="Send your first invoice"
            body="Create an invoice, send it to your customer, and we'll track it until it's paid. Payments match up with your bank deposits automatically."
            action={
              <Link to="/invoices/new" className="btn btn-primary">
                <Plus size={16} /> Create an invoice
              </Link>
            }
          />
        </Card>
      ) : (
        <div className="stack" style={{ gap: 20 }}>
          <div className="grid-4">
            <button className={`card card-pad inv-stat ${filter === "unpaid" ? "active" : ""}`} onClick={() => setFilter("unpaid")}>
              <div className="stat-label">
                <span className="icon-bubble">
                  <Wallet size={17} />
                </span>
                Unpaid
              </div>
              <div className="stat-value num">{money(stats.unpaid)}</div>
              <div className="stat-foot">{stats.unpaidCount ? `Across ${plural(stats.unpaidCount, "invoice")}` : "You're all caught up"}</div>
            </button>
            <button className={`card card-pad inv-stat ${filter === "overdue" ? "active" : ""}`} onClick={() => setFilter("overdue")}>
              <div className="stat-label">
                <span className="icon-bubble neg">
                  <AlertCircle size={17} />
                </span>
                Overdue
              </div>
              <div className={`stat-value num ${stats.overdue > 0 ? "neg" : ""}`}>{money(stats.overdue)}</div>
              <div className={`stat-foot ${stats.overdueCount ? "neg" : ""}`}>
                {stats.overdueCount ? `${plural(stats.overdueCount, "invoice")} past due` : "Nothing past due"}
              </div>
            </button>
            <button className={`card card-pad inv-stat ${filter === "paid" ? "active" : ""}`} onClick={() => setFilter("paid")}>
              <div className="stat-label">
                <span className="icon-bubble pos">
                  <CheckCircle2 size={17} />
                </span>
                Paid in last 30 days
              </div>
              <div className="stat-value num pos">{money(stats.paid30)}</div>
              <div className="stat-foot">{stats.paid30Count ? plural(stats.paid30Count, "payment") : "No payments yet"}</div>
            </button>
            <button className={`card card-pad inv-stat ${filter === "draft" ? "active" : ""}`} onClick={() => setFilter("draft")}>
              <div className="stat-label">
                <span className="icon-bubble violet">
                  <FilePen size={17} />
                </span>
                Drafts
              </div>
              <div className="stat-value num">{stats.drafts}</div>
              <div className="stat-foot">{stats.drafts ? "Not sent yet" : "No drafts waiting"}</div>
            </button>
          </div>

          <Card pad={false}>
            <div className="inv-toolbar">
              <Tabs
                value={filter}
                onChange={setFilter}
                tabs={[
                  { value: "all", label: "All", count: counts.all },
                  { value: "unpaid", label: "Unpaid", count: counts.unpaid },
                  { value: "overdue", label: "Overdue", count: counts.overdue },
                  { value: "paid", label: "Paid", count: counts.paid },
                  { value: "draft", label: "Drafts", count: counts.draft },
                ]}
              />
              <div className="inv-search">
                <Search size={15} />
                <input className="input" placeholder="Search customer or number" value={query} onChange={(e) => setQuery(e.target.value)} />
              </div>
            </div>

            {rows.length === 0 ? (
              <div className="empty" style={{ padding: "40px 24px" }}>
                <div className="empty-title" style={{ fontSize: 15 }}>No invoices here</div>
                <div className="subtle small" style={{ marginTop: 4 }}>
                  {query ? `Nothing matches "${query}".` : "Try another tab."}
                </div>
              </div>
            ) : (
              <div className="table-scroll" role="region" aria-label="Invoices" tabIndex={0}>
              <table className="table">
                <thead>
                  <tr>
                    <th>No.</th>
                    <th>Customer</th>
                    <th>Date</th>
                    <th>Due</th>
                    <th className="right">Total</th>
                    <th className="right">Balance</th>
                    <th>Status</th>
                  </tr>
                </thead>
                <tbody>
                  {rows.map((inv) => {
                    const hint = dueHint(inv, today);
                    return (
                      <tr key={inv.id} className="clickable" onClick={() => navigate(`/invoices/${inv.id}`)}>
                        <td className="num bold">
                          <Link to={`/invoices/${inv.id}`} className="row-link" aria-label={`Invoice #${inv.number} for ${inv.customer_name}`}>
                            #{inv.number}
                          </Link>
                        </td>
                        <td>
                          <div className="row" style={{ gap: 12 }}>
                            <Avatar name={inv.customer_name} />
                            <span className="bold truncate">{inv.customer_name}</span>
                          </div>
                        </td>
                        <td className="muted nowrap">{date(inv.issue_date)}</td>
                        <td className="nowrap">
                          <div>{date(inv.due_date)}</div>
                          {hint && <div className={`inv-due-hint ${hint.cls}`}>{hint.text}</div>}
                        </td>
                        <td className="right">
                          <Money cents={inv.total} />
                        </td>
                        <td className="right bold">
                          {inv.status === "draft" || inv.status === "void" ? <span className="subtle">—</span> : <Money cents={inv.balance} />}
                        </td>
                        <td>
                          <StatusPill status={inv.status} />
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
              </div>
            )}
          </Card>
        </div>
      )}

      <ReceivePaymentModal open={paying} onClose={() => setPaying(false)} />
    </div>
  );
}
