// Savings goals: things the household wants, rather than things it owes.
// Money for a goal lives in a real account; putting money toward it is a
// transfer into that account, so nothing here is make-believe.

import { Pencil, PiggyBank, Plus, Target } from "lucide-react";
import { useEffect, useState } from "react";
import { call, queryClient, useAction, useApi } from "../api";
import { AccountSelect, Card, EmptyState, ErrorBanner, Field, Loading, Modal, MoneyInput, PageHeader, Tabs, useAccountNames, useToast } from "../components/ui";
import { monthName } from "../components/household";
import { celebrate } from "../confetti";
import { date, money, todayISO } from "../format";
import type { Account, Cents, Goal, NewGoal } from "../types";
import "./Goals.css";

/** "On track for March 2027", "Needs $250 a month to make June", and so on. */
function outlook(g: Goal): string {
  // achieved_at is a date, but slicing keeps a full timestamp readable too.
  if (g.achieved_at) return `Reached ${date(g.achieved_at.slice(0, 10))}`;
  if (g.monthly_rate <= 0) return g.required_monthly ? `Needs ${money(g.required_monthly, { whole: true })} a month to make ${date(g.target_date)}` : "Nothing put aside yet";
  const when = g.projected ? monthName(g.projected) : "";
  if (g.on_track === false && g.required_monthly) return `At ${money(g.monthly_rate, { whole: true })} a month you'd get there ${when}; ${date(g.target_date)} needs ${money(g.required_monthly, { whole: true })}`;
  return `At ${money(g.monthly_rate, { whole: true })} a month, you'll get there ${when}`;
}

export default function Goals() {
  const { data, error, isLoading } = useApi<Goal[]>("goals.list");
  const archive = useAction("goals.archive");
  const toast = useToast();
  const accountName = useAccountNames();
  const [editing, setEditing] = useState<Goal | null | "new">(null);
  const [funding, setFunding] = useState<Goal | null>(null);

  if (isLoading) return <Loading />;
  if (error || !data)
    return (
      <div className="page">
        <ErrorBanner error={error} />
      </div>
    );
  // Reached goals stay on the board until someone puts them away (archives them).
  const live = data.filter((g) => !g.archived);

  return (
    <div className="page">
      <PageHeader
        title="Goals"
        subtitle="Things you're saving toward. They add up month after month instead of starting over."
        actions={
          <button className="btn btn-primary" onClick={() => setEditing("new")}>
            <Plus size={16} /> New goal
          </button>
        }
      />
      {live.length === 0 ? (
        <Card>
          <EmptyState
            icon={<Target size={22} />}
            title="No goals yet"
            body="A trip, a cushion for slow months, a new car. Set a target, and Tallybook tells you when you'll get there at the rate you're going."
            action={
              <button className="btn btn-primary" onClick={() => setEditing("new")}>
                Set a goal
              </button>
            }
          />
        </Card>
      ) : (
        <div className="goal-grid">
          {live.map((g) => (
            <div key={g.id} className={`card card-pad goal-card ${g.achieved_at ? "done" : ""}`}>
              <div className="row-between" style={{ gap: 8 }}>
                <div style={{ minWidth: 0 }}>
                  <div className="goal-name truncate">{g.name}</div>
                  <div className="subtle small truncate">
                    In {accountName(g.account_id)}
                    {g.target_date && ` · by ${date(g.target_date)}`}
                  </div>
                </div>
                <button className="btn btn-ghost btn-icon btn-sm" aria-label={`Edit ${g.name}`} onClick={() => setEditing(g)}>
                  <Pencil size={15} />
                </button>
              </div>
              <div className="goal-amount num">
                {money(g.saved, { whole: true })} <span className="subtle">of {money(g.target, { whole: true })}</span>
              </div>
              {/* permille is thousandths; a tenth of it is a percent. */}
              <div className="progress goal-progress" role="progressbar" aria-valuenow={g.permille / 10} aria-valuemin={0} aria-valuemax={100} aria-label={`${g.name} progress`}>
                <div style={{ width: `${g.permille / 10}%` }} />
              </div>
              <div className={`small ${g.on_track === false && !g.achieved_at ? "warn-ink" : "muted"}`}>{outlook(g)}</div>
              <div className="row" style={{ gap: 8, marginTop: "auto", paddingTop: 12 }}>
                {g.achieved_at ? (
                  <button className="btn btn-secondary btn-sm" onClick={() => archive.mutateAsync({ id: g.id, archived: true }).catch((e) => toast.error(e))}>
                    Put it away
                  </button>
                ) : (
                  <button className="btn btn-primary btn-sm" onClick={() => setFunding(g)}>
                    <PiggyBank size={15} /> Put money in
                  </button>
                )}
              </div>
            </div>
          ))}
        </div>
      )}

      <GoalModal goal={editing} onClose={() => setEditing(null)} />
      <ContributeModal goal={funding} onClose={() => setFunding(null)} />
    </div>
  );
}

/** Create a goal (`"new"`) or edit one; `null` keeps the modal closed. */
function GoalModal(props: { goal: Goal | null | "new"; onClose: () => void }) {
  const toast = useToast();
  const accounts = useApi<Account[]>("accounts.list");
  const archive = useAction("goals.archive");
  const open = props.goal !== null;
  const existing = props.goal && props.goal !== "new" ? props.goal : null;
  const [name, setName] = useState("");
  const [target, setTarget] = useState<Cents | null>(null);
  const [targetDate, setTargetDate] = useState("");
  const [account, setAccount] = useState<number | null>(null);
  const [starting, setStarting] = useState<Cents | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<Error | null>(null);

  useEffect(() => {
    if (!open) return;
    setName(existing?.name ?? "");
    setTarget(existing?.target ?? null);
    setTargetDate(existing?.target_date ?? "");
    // Default to a household account with "saving" in its name.
    const savings = (accounts.data ?? []).find((a) => a.scope === "household" && a.kind === "asset" && /sav/i.test(a.name));
    setAccount(existing?.account_id ?? savings?.id ?? null);
    setStarting(existing?.starting ?? null);
    setError(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, existing]);

  const ready = !!name.trim() && !!target && target > 0 && account != null;
  const save = async () => {
    if (!ready) return;
    setBusy(true);
    const goal: NewGoal = { name: name.trim(), target: target!, target_date: targetDate || null, account_id: account!, starting: starting ?? 0, note: existing?.note ?? "" };
    try {
      if (existing) await call("goals.update", { id: existing.id, goal });
      else await call("goals.create", goal);
      await queryClient.invalidateQueries();
      props.onClose();
    } catch (e) {
      setError(e as Error);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open={open}
      onClose={props.onClose}
      title={existing ? "Edit goal" : "New goal"}
      footer={
        <>
          {existing && (
            <button
              className="btn btn-ghost"
              style={{ marginRight: "auto" }}
              onClick={() =>
                archive
                  .mutateAsync({ id: existing.id, archived: true })
                  .then(props.onClose)
                  .catch((e) => toast.error(e))
              }
            >
              Archive
            </button>
          )}
          <button className="btn btn-secondary" onClick={props.onClose}>
            Cancel
          </button>
          <button className="btn btn-primary" disabled={!ready || busy} onClick={save}>
            {busy ? "Saving…" : "Save goal"}
          </button>
        </>
      }
    >
      <div className="stack">
        <ErrorBanner error={error} />
        <Field label="What are you saving for?">
          <input className="input" autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. Hawaii" />
        </Field>
        <div className="form-grid">
          <Field label="How much">
            <MoneyInput value={target} onChange={setTarget} />
          </Field>
          <Field label="By when (optional)" hint="Leave blank for someday.">
            <input className="input" type="date" value={targetDate} onChange={(e) => setTargetDate(e.target.value)} />
          </Field>
        </div>
        <div className="form-grid">
          <Field label="Kept in">
            <AccountSelect scope="household" kinds={["asset"]} value={account} onChange={setAccount} placeholder="e.g. Savings" />
          </Field>
          <Field label="Already saved (optional)">
            <MoneyInput value={starting} onChange={setStarting} />
          </Field>
        </div>
      </div>
    </Modal>
  );
}

/**
 * Put money toward a goal, or take some back: a transfer between a bank
 * account and the goal's account. The backend says when this one reached the
 * target, which is the only time confetti fires.
 */
function ContributeModal(props: { goal: Goal | null; onClose: () => void }) {
  const toast = useToast();
  const accounts = useApi<Account[]>("accounts.list");
  const [amount, setAmount] = useState<Cents | null>(null);
  const [from, setFrom] = useState<number | null>(null);
  const [way, setWay] = useState<"in" | "out">("in");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  const g = props.goal;

  useEffect(() => {
    if (!g) return;
    setAmount(null);
    setWay("in");
    setError(null);
    // Default to the first household bank account that isn't the goal's own.
    const checking = (accounts.data ?? []).find((a) => a.scope === "household" && a.subtype === "bank" && a.id !== g.account_id && !a.archived);
    setFrom(checking?.id ?? null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [g]);

  const save = async () => {
    if (!g || !amount || from == null) return;
    setBusy(true);
    try {
      const r = await call<{ finished: boolean }>("goals.contribute", { id: g.id, from, amount: way === "in" ? amount : -amount, date: todayISO() });
      await queryClient.invalidateQueries();
      if (r.finished) {
        celebrate();
        toast({ text: `You reached ${g.name}! 🎉` });
      } else {
        toast({ text: way === "in" ? `${money(amount)} put toward ${g.name}` : `${money(amount)} taken from ${g.name}` });
      }
      props.onClose();
    } catch (e) {
      setError(e as Error);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open={g !== null}
      onClose={props.onClose}
      title={g ? `Put money toward ${g.name}` : ""}
      subtitle="Moves it into the goal's account, so your books match where the money really is."
      footer={
        <>
          <button className="btn btn-secondary" onClick={props.onClose}>
            Cancel
          </button>
          <button className="btn btn-primary" disabled={!amount || from == null || busy} onClick={save}>
            {busy ? "Saving…" : way === "in" ? "Put it in" : "Take it out"}
          </button>
        </>
      }
    >
      <div className="stack">
        <ErrorBanner error={error} />
        <Tabs<"in" | "out">
          value={way}
          onChange={setWay}
          tabs={[
            { value: "in", label: "Put in" },
            { value: "out", label: "Take out" },
          ]}
        />
        <Field label="How much">
          <MoneyInput value={amount} onChange={setAmount} autoFocus />
        </Field>
        <Field label={way === "in" ? "From" : "Back to"}>
          <AccountSelect scope="household" subtypes={["bank", "other_current_asset"]} exclude={g ? [g.account_id] : []} value={from} onChange={setFrom} />
        </Field>
        {g && g.remaining > 0 && way === "in" && <div className="subtle small">{money(g.remaining)} to go.</div>}
      </div>
    </Modal>
  );
}
