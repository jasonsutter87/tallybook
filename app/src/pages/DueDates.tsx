// Household bills: what's due today, this week, and later; what's been paid;
// and every bill, with repeat payments offered as new ones.

import { CalendarClock, Pencil, Plus, Sparkles } from "lucide-react";
import { useState } from "react";
import { useAction, useApi } from "../api";
import { DueList, groupDue } from "../components/DueList";
import { Card, EmptyState, ErrorBanner, Loading, PageHeader, useAccountNames, useToast } from "../components/ui";
import RecurringBillModal, { type BillDraft } from "../flows/RecurringBillModal";
import { date, money, todayISO } from "../format";
import type { BillSuggestion, Cadence, DueDate, RecurringBill } from "../types";

function cadenceLabel(c: Cadence): string {
  const nth = (n: number) => `${n}${n % 10 === 1 && n !== 11 ? "st" : n % 10 === 2 && n !== 12 ? "nd" : n % 10 === 3 && n !== 13 ? "rd" : "th"}`;
  if (c.every === "week") return "Weekly";
  if (c.every === "months") return c.months === 12 ? `Yearly on the ${nth(c.day)}` : `Every ${c.months} months on the ${nth(c.day)}`;
  return `Monthly on the ${nth(c.day)}`;
}

export default function DueDates() {
  const toast = useToast();
  const today = todayISO();
  const due = useApi<DueDate[]>("recurring.due");
  const bills = useApi<RecurringBill[]>("recurring.list");
  const suggestions = useApi<BillSuggestion[]>("recurring.suggestions");
  const setActive = useAction("recurring.set_active");
  const accountName = useAccountNames();
  const [editing, setEditing] = useState<BillDraft | RecurringBill | null>(null);
  const [open, setOpen] = useState(false);

  const edit = (b: BillDraft | RecurringBill | null) => {
    setEditing(b);
    setOpen(true);
  };

  if (due.isLoading || bills.isLoading) return <Loading />;
  if (due.error || bills.error)
    return (
      <div className="page">
        <ErrorBanner error={due.error ?? bills.error} />
      </div>
    );

  const g = groupDue(due.data ?? [], today);
  const sum = (xs: DueDate[]) => xs.reduce((n, d) => n + d.amount, 0);
  const active = (bills.data ?? []).filter((b) => b.active);
  const stopped = (bills.data ?? []).filter((b) => !b.active);

  return (
    <div className="page">
      <PageHeader
        title="Bills & due dates"
        subtitle="Bills tick themselves off when the payment shows up from your bank."
        actions={
          <button className="btn btn-primary" onClick={() => edit(null)}>
            <Plus size={16} /> Add a bill
          </button>
        }
      />

      <div className="stack" style={{ gap: 20 }}>
        {!!suggestions.data?.length && (
          <Card title={<span className="row" style={{ gap: 8 }}><Sparkles size={16} /> These look like bills</span>}>
            <div className="stack-sm">
              {suggestions.data.map((s) => (
                <div key={`${s.match_text}:${s.account_id}`} className="row-between" style={{ gap: 12, flexWrap: "wrap" }}>
                  <div style={{ minWidth: 0 }}>
                    <div className="bold truncate">{s.name}</div>
                    <div className="subtle small">
                      {money(s.amount)} around the {s.day}
                      {s.day === 1 ? "st" : s.day === 2 ? "nd" : s.day === 3 ? "rd" : "th"} · {accountName(s.account_id)} · seen {s.months_seen} months running
                    </div>
                  </div>
                  <button
                    className="btn btn-secondary btn-sm"
                    onClick={() =>
                      edit({
                        name: s.name,
                        amount: s.amount,
                        account_id: s.account_id,
                        paid_from_id: s.paid_from_id,
                        cadence: { every: "month", day: s.day },
                        match_text: s.match_text,
                      })
                    }
                  >
                    <Plus size={14} /> Add as a bill
                  </button>
                </div>
              ))}
            </div>
          </Card>
        )}

        {active.length === 0 ? (
          <Card>
            <EmptyState
              icon={<CalendarClock size={22} />}
              title="No bills yet"
              body="Add rent, the phone, insurance and the rest, and you'll see what's due today and this week at a glance."
              action={
                <button className="btn btn-primary" onClick={() => edit(null)}>
                  Add a bill
                </button>
              }
            />
          </Card>
        ) : (
          <>
            <div className="grid-3">
              <Stat label="Due now" cents={sum(g.now)} count={g.now.length} tone={g.now.some((d) => d.state === "overdue") ? "neg" : "warn"} />
              <Stat label="Next 7 days" cents={sum(g.week)} count={g.week.length} tone="" />
              <Stat label="Later" cents={sum(g.later)} count={g.later.length} tone="" />
            </div>
            <Card title="Today">
              <DueList items={g.now} empty="Nothing due today. 🎉" />
            </Card>
            <Card title="This week">
              <DueList items={g.week} empty="Nothing else due this week." />
            </Card>
            {g.later.length > 0 && (
              <Card title="Later">
                <DueList items={g.later} />
              </Card>
            )}
            {g.done.length > 0 && (
              <Card title="Paid lately">
                <DueList items={g.done} />
              </Card>
            )}
          </>
        )}

        {active.length > 0 && (
          <Card title="All bills">
            <div className="stack-sm">
              {[...active, ...stopped].map((b) => (
                <div key={b.id} className={`row-between ${b.active ? "" : "muted"}`} style={{ gap: 12 }}>
                  <div style={{ minWidth: 0 }}>
                    <div className="bold truncate">
                      {b.name} {!b.active && <span className="pill">Stopped</span>}
                    </div>
                    <div className="subtle small">
                      {cadenceLabel(b.cadence)} · {b.amount ? money(b.amount) : "varies"} · {accountName(b.account_id)}
                      {b.next_due && ` · next ${date(b.next_due)}`}
                    </div>
                  </div>
                  <div className="row" style={{ gap: 4 }}>
                    <button className="btn btn-ghost btn-sm" onClick={() => setActive.mutateAsync({ id: b.id, active: !b.active }).catch((e) => toast.error(e))}>
                      {b.active ? "Stop" : "Restart"}
                    </button>
                    <button className="btn btn-ghost btn-icon btn-sm" aria-label={`Edit ${b.name}`} onClick={() => edit(b)}>
                      <Pencil size={15} />
                    </button>
                  </div>
                </div>
              ))}
            </div>
          </Card>
        )}
      </div>

      <RecurringBillModal open={open} onClose={() => setOpen(false)} bill={editing} />
    </div>
  );
}

function Stat(props: { label: string; cents: number; count: number; tone: string }) {
  return (
    <div className="card card-pad">
      <div className="stat-label">{props.label}</div>
      <div className={`stat-value num ${props.count && props.tone ? props.tone : ""}`}>{money(props.cents, { whole: true })}</div>
      <div className="stat-foot">{props.count === 1 ? "1 bill" : `${props.count} bills`}</div>
    </div>
  );
}
