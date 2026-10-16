import { AlertCircle, CalendarClock, CheckCircle2, Plus, Receipt, Search, Wallet } from "lucide-react";
import { useMemo, useState, type ReactNode } from "react";
import { useNavigate } from "react-router-dom";
import { useApi } from "../api";
import { Avatar, EmptyState, ErrorBanner, Loading, Money, PageHeader, StatusPill, Tabs } from "../components/ui";
import { addDays, date, money, plural, startOfMonth } from "../format";
import PayBillsModal from "../flows/PayBillsModal";
import type { BillSummary, PaymentSummary } from "../types";
import { DueHint, isOverdue, isUnpaid, useToday } from "./billsShared";
import "./bills.css";

type Tab = "unpaid" | "overdue" | "paid" | "all";

export default function Bills() {
  const navigate = useNavigate();
  const today = useToday();
  const { data, error, isLoading } = useApi<BillSummary[]>("bills.list");
  const paidThisMonth = usePaidThisMonth(today);
  const [tab, setTab] = useState<Tab>("unpaid");
  const [query, setQuery] = useState("");
  const [paying, setPaying] = useState(false);

  const bills = data ?? [];

  const stats = useMemo(() => {
    const unpaid = bills.filter(isUnpaid);
    const overdue = unpaid.filter((b) => isOverdue(b, today));
    const weekOut = addDays(today, 7);
    const soon = unpaid.filter((b) => !isOverdue(b, today) && b.due_date <= weekOut);
    const sum = (xs: BillSummary[]) => xs.reduce((s, b) => s + b.balance, 0);
    return {
      unpaid,
      overdue,
      soon,
      toPay: sum(unpaid),
      overdueAmt: sum(overdue),
      soonAmt: sum(soon),
      paid: bills.filter((b) => b.status === "paid"),
    };
  }, [bills, today]);

  const rows = useMemo(() => {
    const base =
      tab === "unpaid" ? stats.unpaid : tab === "overdue" ? stats.overdue : tab === "paid" ? stats.paid : bills;
    const q = query.trim().toLowerCase();
    const filtered = q ? base.filter((b) => `${b.vendor_name} ${b.number} ${b.memo}`.toLowerCase().includes(q)) : base;
    // Unpaid views read best soonest-due first; history views newest first.
    return tab === "unpaid" || tab === "overdue"
      ? [...filtered].sort((a, b) => a.due_date.localeCompare(b.due_date))
      : filtered;
  }, [tab, stats, bills, query]);

  if (isLoading) return <Loading />;

  return (
    <div className="page">
      <PageHeader
        title="Bills"
        subtitle="Money you owe vendors, and when it's due."
        actions={
          <>
            <button className="btn btn-secondary" onClick={() => setPaying(true)} disabled={!stats.unpaid.length}>
              <Wallet size={16} /> Pay bills
            </button>
            <button className="btn btn-primary" onClick={() => navigate("/bills/new")}>
              <Plus size={16} /> Enter a bill
            </button>
          </>
        }
      />

      <div className="stack" style={{ gap: 20 }}>
        <ErrorBanner error={error} />

        <div className="grid-4">
          <Stat icon={<Receipt size={18} />} tone="" label="To pay" cents={stats.toPay} foot={stats.unpaid.length ? plural(stats.unpaid.length, "unpaid bill") : "Nothing outstanding"} onClick={() => setTab("unpaid")} />
          <Stat
            icon={<AlertCircle size={18} />}
            tone="neg"
            label="Overdue"
            cents={stats.overdueAmt}
            valueColor={stats.overdueAmt ? "var(--neg)" : undefined}
            foot={stats.overdue.length ? `${plural(stats.overdue.length, "bill")} past due` : "Nothing late"}
            onClick={() => setTab("overdue")}
          />
          <Stat
            icon={<CalendarClock size={18} />}
            tone="warn"
            label="Due in 7 days"
            cents={stats.soonAmt}
            valueColor={stats.soonAmt ? "var(--warn)" : undefined}
            foot={stats.soon.length ? plural(stats.soon.length, "bill") : "Nothing due this week"}
            onClick={() => setTab("unpaid")}
          />
          <Stat icon={<CheckCircle2 size={18} />} tone="pos" label="Paid this month" cents={paidThisMonth} foot="Payments to vendors" onClick={() => setTab("paid")} />
        </div>

        <div className="card">
          <div className="row-between" style={{ padding: "16px 18px", flexWrap: "wrap", gap: 12 }}>
            <Tabs
              value={tab}
              onChange={setTab}
              tabs={[
                { value: "unpaid", label: "Unpaid", count: stats.unpaid.length },
                { value: "overdue", label: "Overdue", count: stats.overdue.length },
                { value: "paid", label: "Paid", count: stats.paid.length },
                { value: "all", label: "All", count: bills.length },
              ]}
            />
            <div className="bk-search">
              <Search size={15} />
              <input className="input" placeholder="Search vendor or bill #" value={query} onChange={(e) => setQuery(e.target.value)} />
            </div>
          </div>

          {rows.length === 0 ? (
            <Empty tab={tab} searching={!!query.trim()} hasAny={bills.length > 0} onNew={() => navigate("/bills/new")} />
          ) : (
            <table className="table">
              <thead>
                <tr>
                  <th>Vendor</th>
                  <th>Bill #</th>
                  <th>Bill date</th>
                  <th>Due</th>
                  <th className="right">Total</th>
                  <th className="right">Balance</th>
                  <th>Status</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((b) => (
                  <tr key={b.id} className="clickable" onClick={() => navigate(`/bills/${b.id}`)} style={b.status === "void" ? { opacity: 0.5 } : undefined}>
                    <td>
                      <div className="row" style={{ gap: 12 }}>
                        <Avatar name={b.vendor_name} />
                        <div style={{ minWidth: 0 }}>
                          <div className="bold truncate">{b.vendor_name}</div>
                          {b.memo && <div className="small subtle truncate" style={{ maxWidth: 240 }}>{b.memo}</div>}
                        </div>
                      </div>
                    </td>
                    <td className="muted">{b.number || "—"}</td>
                    <td className="muted nowrap">{date(b.issue_date)}</td>
                    <td className="nowrap">
                      {date(b.due_date)}
                      <DueHint bill={b} today={today} />
                    </td>
                    <td className="right">
                      <Money cents={b.total} />
                    </td>
                    <td className="right bold">{b.status === "void" ? <span className="subtle">—</span> : <Money cents={b.balance} />}</td>
                    <td>
                      <StatusPill status={b.status} />
                    </td>
                  </tr>
                ))}
              </tbody>
              {(tab === "unpaid" || tab === "overdue") && rows.length > 1 && (
                <tfoot>
                  <tr>
                    <td colSpan={5} className="muted">
                      Total
                    </td>
                    <td className="right">{money(rows.reduce((s, b) => s + b.balance, 0))}</td>
                    <td />
                  </tr>
                </tfoot>
              )}
            </table>
          )}
        </div>
      </div>

      <PayBillsModal open={paying} onClose={() => setPaying(false)} />
    </div>
  );
}

/** Sum of non-voided payments to vendors dated this month. */
function usePaidThisMonth(today: string): number {
  const payments = useApi<PaymentSummary[]>("payments.list", { kind: "made", from: startOfMonth(today), to: today });
  return useMemo(() => (payments.data ?? []).filter((p) => !p.voided).reduce((s, p) => s + p.amount, 0), [payments.data]);
}

function Stat(props: { icon: ReactNode; tone: string; label: string; cents: number; foot: string; valueColor?: string; onClick?: () => void }) {
  return (
    <button className="card card-pad" onClick={props.onClick} style={{ textAlign: "left", cursor: "pointer", font: "inherit", color: "inherit" }}>
      <div className="stat-label">
        <span className={`icon-bubble ${props.tone}`}>{props.icon}</span>
        {props.label}
      </div>
      <div className="stat-value num" style={props.valueColor ? { color: props.valueColor } : undefined}>
        {money(props.cents)}
      </div>
      <div className="stat-foot">{props.foot}</div>
    </button>
  );
}

function Empty(props: { tab: Tab; searching: boolean; hasAny: boolean; onNew: () => void }) {
  if (props.searching) return <EmptyState icon={<Search size={22} />} title="No matching bills" body="Try a different vendor name or bill number." />;
  if (!props.hasAny)
    return (
      <EmptyState
        icon={<Receipt size={22} />}
        title="No bills yet"
        body="When a vendor sends you a bill, enter it here. Tallybook keeps track of what you owe and when it's due."
        action={
          <button className="btn btn-primary" onClick={props.onNew}>
            <Plus size={16} /> Enter your first bill
          </button>
        }
      />
    );
  if (props.tab === "unpaid") return <EmptyState icon={<CheckCircle2 size={22} />} title="All bills paid" body="Nice — you don't owe any vendors right now." />;
  if (props.tab === "overdue") return <EmptyState icon={<CheckCircle2 size={22} />} title="Nothing overdue" body="Every unpaid bill is still on time." />;
  return <EmptyState icon={<Receipt size={22} />} title="Nothing here yet" body="Paid bills will show up here." />;
}
