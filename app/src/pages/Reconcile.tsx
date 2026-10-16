// Reconcile an account against a bank or card statement: tick off what the
// statement shows until the difference is zero, then lock it in.

import { useQueryClient } from "@tanstack/react-query";
import { CalendarCheck, ClipboardCheck, History, PartyPopper, RotateCcw } from "lucide-react";
import { useMemo, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { call, useAction, useApi } from "../api";
import { ConfirmModal, EmptyState, ErrorBanner, Field, Loading, Money, MoneyInput, PageHeader, useToast } from "../components/ui";
import { date, money, plural, shortDate, todayISO } from "../format";
import type { Account, Cents, ReconcileCandidate, ReconcileSummary, Reconciliation } from "../types";
import "./accounts.css";

export default function Reconcile() {
  const { id: idParam } = useParams();
  const accountId = Number(idParam);
  const accounts = useApi<Account[]>("accounts.list");
  const active = useApi<Reconciliation | null>("reconcile.active", { account_id: accountId });
  const account = accounts.data?.find((a) => a.id === accountId);

  if (accounts.isLoading || active.isLoading) return <Loading />;
  if (!account)
    return (
      <div className="page">
        <PageHeader title="Account not found" back={{ to: "/accounts", label: "Accounts" }} />
        <ErrorBanner error={accounts.error} />
      </div>
    );

  return (
    <div className="page">
      <PageHeader
        back={{ to: `/accounts/${accountId}`, label: account.name }}
        title={`Reconcile ${account.name}`}
        subtitle={
          active.data
            ? `Statement ending ${date(active.data.statement_date)} · ${account.kind === "liability" ? "amount owed" : "balance"} ${money(active.data.statement_balance)}`
            : "Make sure your books match your statement, line for line."
        }
      />
      <ErrorBanner error={active.error} />
      {!account.reconcilable ? (
        <EmptyState icon={<ClipboardCheck size={22} />} title="This account can't be reconciled" body="Only bank, card and similar balance-sheet accounts have statements to match." />
      ) : active.data ? (
        <InProgress account={account} rec={active.data} />
      ) : (
        <Start account={account} />
      )}
    </div>
  );
}

// ------------------------------------------------------------- start

function Start({ account }: { account: Account }) {
  const toast = useToast();
  const history = useApi<Reconciliation[]>("reconcile.history", { account_id: account.id });
  const start = useAction<number>("reconcile.start");
  const undo = useAction<void>("reconcile.undo_last");
  const [stmtDate, setStmtDate] = useState(todayISO());
  const [balance, setBalance] = useState<Cents | null>(null);
  const [confirmUndo, setConfirmUndo] = useState(false);
  const liability = account.kind === "liability";
  const last = history.data?.[0];

  const begin = async () => {
    if (balance == null) return;
    try {
      await start.mutateAsync({ account_id: account.id, statement_date: stmtDate, statement_balance: balance });
    } catch {
      /* inline */
    }
  };

  return (
    <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1.3fr) minmax(0, 1fr)", gap: 20, alignItems: "start" }}>
      <section className="card card-pad">
        <div className="stack">
          <div className="row" style={{ gap: 12 }}>
            <span className="icon-bubble">
              <CalendarCheck size={18} />
            </span>
            <div>
              <div className="card-title">Start with your statement</div>
              <div className="muted small">Grab your latest {liability ? "card" : "bank"} statement and enter its ending numbers.</div>
            </div>
          </div>
          <ErrorBanner error={start.error} />
          <form
            className="stack"
            onSubmit={(e) => {
              e.preventDefault();
              begin();
            }}
          >
            <Field label="Statement ending date" hint={last ? `Last reconciled through ${date(last.statement_date)}` : "The last day the statement covers"}>
              <input className="input" type="date" value={stmtDate} min={last?.statement_date} onChange={(e) => setStmtDate(e.target.value)} />
            </Field>
            <Field
              label={liability ? "Statement balance (amount owed)" : "Statement ending balance"}
              hint={liability ? "The new balance on the statement — what you owed at the end" : "The ending balance printed on the statement"}
            >
              <MoneyInput value={balance} onChange={setBalance} large allowNegative autoFocus />
            </Field>
            <div className="row-between" style={{ marginTop: 4 }}>
              <span className="subtle small">Your books say {money(account.balance)} today.</span>
              <button type="submit" className="btn btn-primary btn-lg" disabled={balance == null || !stmtDate || start.isPending}>
                {start.isPending ? "Starting…" : "Start reconciling"}
              </button>
            </div>
          </form>
        </div>
      </section>

      <section className="card">
        <div className="card-header">
          <h3 className="card-title row">
            <History size={16} /> Past statements
          </h3>
        </div>
        <div style={{ paddingTop: 8 }}>
          {history.isLoading ? (
            <Loading />
          ) : !history.data?.length ? (
            <div className="card-body muted">No statements reconciled yet. Your first one sets the starting point.</div>
          ) : (
            <>
              {history.data.map((r, i) => (
                <div className="list-row" key={r.id}>
                  <div className="grow">
                    <div className="bold">{date(r.statement_date)}</div>
                    {i === 0 && <div className="subtle small">Most recent</div>}
                  </div>
                  <Money cents={r.statement_balance} className="bold" />
                </div>
              ))}
              <div className="list-row">
                <button className="btn btn-ghost btn-sm" onClick={() => setConfirmUndo(true)}>
                  <RotateCcw size={14} /> Undo last reconciliation
                </button>
              </div>
            </>
          )}
        </div>
      </section>

      <ConfirmModal
        open={confirmUndo}
        onClose={() => setConfirmUndo(false)}
        title="Undo the last reconciliation?"
        confirmLabel="Undo reconciliation"
        danger
        body={
          <>
            This reopens the statement ending {last ? date(last.statement_date) : ""}. Its transactions go back to "cleared" so you can fix them and
            reconcile again. Nothing is deleted.
          </>
        }
        onConfirm={async () => {
          try {
            await undo.mutateAsync({ account_id: account.id });
            toast({ text: "Reconciliation reopened" });
          } catch (e) {
            toast.error(e);
          }
        }}
      />
    </div>
  );
}

// ------------------------------------------------------------- in progress

function InProgress({ account, rec }: { account: Account; rec: Reconciliation }) {
  const toast = useToast();
  const navigate = useNavigate();
  const qc = useQueryClient();
  const candidates = useApi<ReconcileCandidate[]>("reconcile.candidates", { id: rec.id });
  const summary = useApi<ReconcileSummary>("reconcile.summary", { id: rec.id });
  const finish = useAction<void>("reconcile.finish");
  const cancel = useAction<void>("reconcile.cancel");
  const [pending, setPending] = useState<Record<number, boolean>>({});
  const [busy, setBusy] = useState(false);
  const [confirmCancel, setConfirmCancel] = useState(false);
  const liability = account.kind === "liability";

  const refresh = () =>
    Promise.all([
      qc.invalidateQueries({ queryKey: ["reconcile.candidates"] }),
      qc.invalidateQueries({ queryKey: ["reconcile.summary"] }),
      qc.invalidateQueries({ queryKey: ["accounts.register"] }),
    ]);

  const setChecked = async (lines: ReconcileCandidate[], checked: boolean) => {
    const changed = lines.filter((l) => (pending[l.line_id] ?? l.checked) !== checked);
    if (!changed.length) return;
    setPending((p) => ({ ...p, ...Object.fromEntries(changed.map((l) => [l.line_id, checked])) }));
    if (changed.length > 1) setBusy(true);
    try {
      await call("reconcile.toggle_many", { id: rec.id, line_ids: changed.map((l) => l.line_id), checked });
    } catch (e) {
      toast.error(e);
    } finally {
      await refresh();
      setPending((p) => {
        const next = { ...p };
        for (const l of changed) delete next[l.line_id];
        return next;
      });
      setBusy(false);
    }
  };

  const isChecked = (c: ReconcileCandidate) => pending[c.line_id] ?? c.checked;

  const [left, right] = useMemo(() => {
    const all = candidates.data ?? [];
    // Asset: withdrawals (−) | deposits (+). Liability: charges (+) | payments (−).
    return liability ? [all.filter((c) => c.amount > 0), all.filter((c) => c.amount <= 0)] : [all.filter((c) => c.amount < 0), all.filter((c) => c.amount >= 0)];
  }, [candidates.data, liability]);

  const s = summary.data;
  const balanced = s?.difference === 0;
  const anyPending = busy || Object.keys(pending).length > 0;

  const doFinish = async () => {
    try {
      await finish.mutateAsync({ id: rec.id });
      toast({ text: `🎉 ${account.name} is reconciled through ${shortDate(rec.statement_date)}` });
      navigate(`/accounts/${account.id}`);
    } catch (e) {
      toast.error(e);
    }
  };

  if (candidates.isLoading || summary.isLoading) return <Loading />;

  return (
    <div className="stack" style={{ gap: 0 }}>
      <ErrorBanner error={candidates.error ?? summary.error} />
      <div className="rec-cols">
        <Column
          title={liability ? "Charges" : "Payments & withdrawals"}
          lines={left}
          isChecked={isChecked}
          onToggle={setChecked}
          disabled={busy}
          tone={liability ? "" : "neg"}
        />
        <Column
          title={liability ? "Payments & credits" : "Deposits"}
          lines={right}
          isChecked={isChecked}
          onToggle={setChecked}
          disabled={busy}
          tone={liability ? "" : "pos"}
        />
      </div>

      <div className="rec-bar">
        {s && (
          <>
            <Stat label="Beginning balance" cents={s.beginning_balance} />
            <span className="rec-op">{s.cleared_total < 0 ? "−" : "+"}</span>
            <Stat label="Cleared" cents={Math.abs(s.cleared_total)} />
            <span className="rec-op">=</span>
            <Stat label="Cleared balance" cents={s.cleared_balance} />
            <span className="rec-op" style={{ opacity: 0.5 }}>
              vs
            </span>
            <Stat label={liability ? "Statement (owed)" : "Statement balance"} cents={s.statement_balance} />
            <div className={`rec-diff ${balanced ? "ok" : "off"}`}>
              <div className="small bold">Difference</div>
              <div className="v num">{balanced ? "$0.00 — balanced! 🎉" : money(s.difference)}</div>
            </div>
          </>
        )}
        <div className="row" style={{ width: "100%", justifyContent: "flex-end", flexWrap: "wrap" }}>
          <button className="btn btn-ghost" style={{ marginRight: "auto" }} onClick={() => setConfirmCancel(true)}>
            Cancel reconciliation
          </button>
          <button className="btn btn-secondary" onClick={() => navigate(`/accounts/${account.id}`)}>
            Save for later
          </button>
          <button
            className="btn btn-primary btn-lg"
            disabled={!balanced || anyPending || finish.isPending}
            onClick={doFinish}
            title={balanced ? undefined : "The difference needs to be $0.00 to finish"}
          >
            <PartyPopper size={16} /> {finish.isPending ? "Finishing…" : "Finish"}
          </button>
        </div>
      </div>

      <ConfirmModal
        open={confirmCancel}
        onClose={() => setConfirmCancel(false)}
        title="Cancel this reconciliation?"
        confirmLabel="Cancel reconciliation"
        danger
        body="Your statement session is discarded. Transactions you ticked stay marked as cleared, so you won't lose that work."
        onConfirm={async () => {
          try {
            await cancel.mutateAsync({ id: rec.id });
            toast({ text: "Reconciliation cancelled" });
          } catch (e) {
            toast.error(e);
          }
        }}
      />
    </div>
  );
}

function Stat({ label, cents }: { label: string; cents: Cents }) {
  return (
    <div className="rec-stat">
      <span className="label">{label}</span>
      <span className="v num">{money(cents)}</span>
    </div>
  );
}

function Column(props: {
  title: string;
  lines: ReconcileCandidate[];
  isChecked: (c: ReconcileCandidate) => boolean;
  onToggle: (lines: ReconcileCandidate[], checked: boolean) => void;
  disabled: boolean;
  tone: string;
}) {
  const checked = props.lines.filter(props.isChecked);
  const total = checked.reduce((s, c) => s + Math.abs(c.amount), 0);
  const allChecked = props.lines.length > 0 && checked.length === props.lines.length;
  return (
    <section className="card">
      <div className="acct-section-head">
        <div>
          <div className="card-title">{props.title}</div>
          <div className="subtle small">
            {checked.length} of {plural(props.lines.length, "item")} checked · {money(total)}
          </div>
        </div>
        {props.lines.length > 0 && (
          <button className="btn btn-ghost btn-sm" disabled={props.disabled} onClick={() => props.onToggle(props.lines, !allChecked)}>
            {allChecked ? "Uncheck all" : "Check all"}
          </button>
        )}
      </div>
      {props.lines.length === 0 ? (
        <div className="card-body muted small">Nothing here on or before the statement date.</div>
      ) : (
        <div className="rec-list">
          {props.lines.map((c) => {
            const on = props.isChecked(c);
            return (
              <label key={c.line_id} className={`rec-row ${on ? "checked" : ""}`} style={{ borderTop: undefined }}>
                <input
                  type="checkbox"
                  className="checkbox"
                  checked={on}
                  disabled={props.disabled}
                  onChange={(e) => props.onToggle([c], e.target.checked)}
                />
                <span className="muted nowrap small" style={{ width: 56 }}>
                  {shortDate(c.date)}
                </span>
                <span className="grow truncate">{c.memo || <span className="subtle">No description</span>}</span>
                <span className={`num bold ${props.tone}`}>{money(Math.abs(c.amount))}</span>
              </label>
            );
          })}
        </div>
      )}
    </section>
  );
}
