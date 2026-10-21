// Edit a month's plan: what there is to work with, and a line per person or
// category. Saving gives the month its own plan, which then carries forward.

import { Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { call, queryClient } from "../api";
import { monthName, usePeople } from "../components/household";
import { AccountSelect, ErrorBanner, Field, Modal, MoneyInput, Tabs, useAccountNames, useToast } from "../components/ui";
import { FLEX_LABEL } from "../format";
import type { Account, BudgetDirection, BudgetTarget, Cents, MonthBudget } from "../types";
import { useApi } from "../api";

interface Draft {
  target: BudgetTarget;
  amount: Cents | null;
  direction: BudgetDirection;
  tolerance_pct: number;
}

const key = (t: BudgetTarget) => `${t.kind}:${t.id}`;

export default function PlanModal(props: { open: boolean; onClose: () => void; month: MonthBudget }) {
  const toast = useToast();
  const { people } = usePeople();
  const accounts = useApi<Account[]>("accounts.list");
  const accountName = useAccountNames();
  const [available, setAvailable] = useState<Cents | null>(null);
  const [lines, setLines] = useState<Draft[]>([]);
  const [adding, setAdding] = useState<"account" | "person">("account");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<Error | null>(null);

  useEffect(() => {
    if (!props.open) return;
    setAvailable(props.month.available);
    setLines(props.month.lines.map((l) => ({ target: l.target, amount: l.amount, direction: l.direction, tolerance_pct: l.tolerance_pct })));
    setError(null);
  }, [props.open, props.month]);

  const nameOf = (t: BudgetTarget) => (t.kind === "person" ? (people.find((p) => p.id === t.id)?.name ?? "Someone") : accountName(t.id));
  const accountOf = (id: number) => accounts.data?.find((a) => a.id === id);
  const set = (i: number, patch: Partial<Draft>) => setLines((ls) => ls.map((l, j) => (j === i ? { ...l, ...patch } : l)));
  const add = (target: BudgetTarget) => {
    if (lines.some((l) => key(l.target) === key(target))) return;
    // Savings accounts and income are goals to reach; spending is a limit.
    const kind = target.kind === "account" ? accountOf(target.id)?.kind : "expense";
    setLines((ls) => [...ls, { target, amount: null, direction: kind === "expense" ? "ceiling" : "floor", tolerance_pct: 10 }]);
  };

  const save = async () => {
    setBusy(true);
    setError(null);
    const month = props.month.month;
    try {
      if (available !== props.month.available || props.month.plan_from) {
        await call("budget.set_available", { month, amount: available });
      }
      const kept = new Set(lines.map((l) => key(l.target)));
      for (const l of props.month.lines) {
        if (!kept.has(key(l.target))) await call("budget.remove", { month, target: l.target });
      }
      for (const l of lines) {
        await call("budget.set", { month, line: { target: l.target, amount: l.amount ?? 0, direction: l.direction, tolerance_pct: l.tolerance_pct } });
      }
      await queryClient.invalidateQueries();
      toast({ text: `Plan for ${monthName(month, false)} saved` });
      props.onClose();
    } catch (e) {
      setError(e as Error);
    } finally {
      setBusy(false);
    }
  };

  const used = new Set(lines.map((l) => key(l.target)));
  return (
    <Modal
      open={props.open}
      onClose={props.onClose}
      width={640}
      title={`Plan for ${monthName(props.month.month)}`}
      subtitle="Each month starts with the plan from the month before. Changes here apply from this month on."
      footer={
        <>
          <button className="btn btn-secondary" onClick={props.onClose}>
            Cancel
          </button>
          <button className="btn btn-primary" disabled={busy} onClick={save}>
            {busy ? "Saving…" : "Save plan"}
          </button>
        </>
      }
    >
      <div className="stack">
        <ErrorBanner error={error} />
        <Field label="What there is to work with this month" hint="Usually what you expect to bring in. Leave blank if you'd rather not say.">
          <MoneyInput value={available} onChange={setAvailable} />
        </Field>

        <div className="label">Budget lines</div>
        {lines.length === 0 && <div className="muted small">No lines yet. Add a person or a category below.</div>}
        <div className="stack-sm">
          {lines.map((l, i) => {
            const acct = l.target.kind === "account" ? accountOf(l.target.id) : undefined;
            return (
              <div key={key(l.target)} className="plan-line">
                <div className="grow" style={{ minWidth: 0 }}>
                  <div className="bold truncate">{nameOf(l.target)}</div>
                  <div className="subtle small">
                    {l.target.kind === "person" ? "Everything for them" : acct?.flex ? FLEX_LABEL[acct.flex] : acct?.kind === "income" ? "Income" : acct?.kind === "asset" ? "Savings" : "Spending"}
                  </div>
                </div>
                <select
                  className="select plan-direction"
                  value={l.direction}
                  disabled={l.target.kind === "person"}
                  onChange={(e) => set(i, { direction: e.target.value as BudgetDirection })}
                  aria-label="Limit or goal"
                >
                  <option value="ceiling">Up to</option>
                  <option value="floor">At least</option>
                </select>
                <div className="plan-amount">
                  <MoneyInput value={l.amount} onChange={(v) => set(i, { amount: v })} />
                </div>
                <button className="btn btn-ghost btn-icon" aria-label={`Remove ${nameOf(l.target)}`} onClick={() => setLines((ls) => ls.filter((_, j) => j !== i))}>
                  <Trash2 size={16} />
                </button>
              </div>
            );
          })}
        </div>

        <div className="plan-add">
          <Tabs<"account" | "person">
            value={adding}
            onChange={setAdding}
            tabs={[
              { value: "account", label: "Category" },
              { value: "person", label: "Person" },
            ]}
          />
          {adding === "account" ? (
            <AccountSelect
              scope="household"
              kinds={["expense", "income", "asset"]}
              prefer={["expense"]}
              exclude={lines.filter((l) => l.target.kind === "account").map((l) => l.target.id)}
              value={null}
              onChange={(id) => id != null && add({ kind: "account", id })}
              placeholder="Add a category, income, or savings account…"
            />
          ) : people.length ? (
            <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
              {people.map((p) => (
                <button key={p.id} className="btn btn-secondary btn-sm" disabled={used.has(`person:${p.id}`)} onClick={() => add({ kind: "person", id: p.id })}>
                  + {p.name}
                </button>
              ))}
            </div>
          ) : (
            <div className="muted small">Add the people in your household first, from "Who it was for" on the month.</div>
          )}
        </div>
      </div>
    </Modal>
  );
}
