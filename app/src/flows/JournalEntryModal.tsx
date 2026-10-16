// Records a manual transaction. "Simple" moves money from one account to
// another; "Journal entry" is the full debit/credit form for accountants.

import { ArrowRight, Plus, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useAction } from "../api";
import { AccountSelect, ErrorBanner, Field, Modal, MoneyInput, Tabs, useToast } from "../components/ui";
import { money, todayISO } from "../format";
import type { Cents, EntryLineInput } from "../types";
import "../pages/accounts.css";

type Mode = "simple" | "journal";
interface Line {
  key: number;
  account_id: number | null;
  debit: Cents | null;
  credit: Cents | null;
  memo: string;
}

let nextKey = 1;
const blank = (account_id: number | null = null): Line => ({ key: nextKey++, account_id, debit: null, credit: null, memo: "" });

export default function JournalEntryModal(props: { open: boolean; onClose: () => void; defaultAccountId?: number; onPosted?: (entryId: number) => void }) {
  const toast = useToast();
  const post = useAction<number>("entries.post");

  const [mode, setMode] = useState<Mode>("simple");
  const [date, setDate] = useState(todayISO());
  const [memo, setMemo] = useState("");
  // simple
  const [amount, setAmount] = useState<Cents | null>(null);
  const [from, setFrom] = useState<number | null>(null);
  const [to, setTo] = useState<number | null>(null);
  // journal
  const [lines, setLines] = useState<Line[]>([]);

  useEffect(() => {
    if (!props.open) return;
    setMode("simple");
    setDate(todayISO());
    setMemo("");
    setAmount(null);
    setFrom(props.defaultAccountId ?? null);
    setTo(null);
    setLines([blank(props.defaultAccountId ?? null), blank()]);
    post.reset();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.open, props.defaultAccountId]);

  const switchMode = (m: Mode) => {
    // Carry a half-filled simple form into the journal grid.
    if (m === "journal" && mode === "simple" && (amount || from || to)) {
      setLines([
        { ...blank(to), debit: amount || null },
        { ...blank(from), credit: amount || null },
      ]);
    }
    setMode(m);
    post.reset();
  };

  const debits = lines.reduce((s, l) => s + (l.debit ?? 0), 0);
  const credits = lines.reduce((s, l) => s + (l.credit ?? 0), 0);
  const filled = lines.filter((l) => l.account_id != null && ((l.debit ?? 0) > 0 || (l.credit ?? 0) > 0));
  const offBy = debits - credits;

  const simpleReady = !!amount && amount > 0 && from != null && to != null && from !== to && !!date;
  const journalReady = filled.length >= 2 && offBy === 0 && debits > 0 && !!date && filled.length === lines.filter((l) => l.account_id != null || l.debit || l.credit).length;
  const ready = mode === "simple" ? simpleReady : journalReady;

  const save = async () => {
    if (!ready) return;
    let out: EntryLineInput[];
    if (mode === "simple") {
      out = [
        { account_id: to!, amount: amount!, memo: null },
        { account_id: from!, amount: -amount!, memo: null },
      ];
    } else {
      out = filled.map((l) => ({
        account_id: l.account_id!,
        amount: (l.debit ?? 0) - (l.credit ?? 0),
        memo: l.memo.trim() || null,
      }));
    }
    try {
      const id = await post.mutateAsync({ date, memo: memo.trim(), lines: out });
      toast({ text: "Transaction recorded" });
      props.onPosted?.(id);
      props.onClose();
    } catch {
      /* shown inline */
    }
  };

  const update = (key: number, patch: Partial<Line>) => setLines((ls) => ls.map((l) => (l.key === key ? { ...l, ...patch } : l)));

  return (
    <Modal
      open={props.open}
      onClose={props.onClose}
      title="Record a transaction"
      subtitle={mode === "simple" ? "For things that didn't come through a bank feed, invoice or bill." : "Full double-entry. Debits must equal credits."}
      width={mode === "journal" ? 760 : 560}
      footer={
        <>
          <button className="btn btn-secondary" onClick={props.onClose}>
            Cancel
          </button>
          <button className="btn btn-primary" disabled={!ready || post.isPending} onClick={save}>
            {post.isPending ? "Saving…" : "Save transaction"}
          </button>
        </>
      }
    >
      <div className="stack">
        <div>
        <Tabs<Mode>
          value={mode}
          onChange={switchMode}
          tabs={[
            { value: "simple", label: "Simple" },
            { value: "journal", label: "Journal entry" },
          ]}
        />
        </div>
        <ErrorBanner error={post.error} />

        <div className="form-grid" style={{ gridTemplateColumns: "minmax(0, 160px) minmax(0, 1fr)" }}>
          <Field label="Date">
            <input className="input" type="date" value={date} onChange={(e) => setDate(e.target.value)} />
          </Field>
          <Field label="Description">
            <input className="input" value={memo} onChange={(e) => setMemo(e.target.value)} placeholder={mode === "simple" ? "e.g. October rent" : "e.g. Year-end depreciation"} />
          </Field>
        </div>

        {mode === "simple" ? (
          <>
            <Field label="Amount">
              <MoneyInput value={amount} onChange={setAmount} large />
            </Field>
            <div className="from-to">
              <Field label="From">
                <AccountSelect value={from} onChange={setFrom} placeholder="Where the money came from" exclude={to != null ? [to] : undefined} />
              </Field>
              <div className="arrow">
                <ArrowRight size={18} />
              </div>
              <Field label="To">
                <AccountSelect value={to} onChange={setTo} placeholder="Where it went" exclude={from != null ? [from] : undefined} />
              </Field>
            </div>
            <div className="hint">
              Money moves from → to. E.g. paid rent: from <b>Checking</b> to <b>Rent</b>. Paid off a card: from <b>Checking</b> to <b>Visa</b>.
            </div>
          </>
        ) : (
          <div className="stack-sm">
            <div className="je-grid je-head">
              <span>Account</span>
              <span className="right">Debit</span>
              <span className="right">Credit</span>
              <span>Line memo</span>
              <span />
            </div>
            {lines.map((l) => (
              <div className="je-grid" key={l.key}>
                <AccountSelect value={l.account_id} onChange={(v) => update(l.key, { account_id: v })} placeholder="Account" />
                <MoneyInput value={l.debit} onChange={(v) => update(l.key, { debit: v, credit: v ? null : l.credit })} />
                <MoneyInput value={l.credit} onChange={(v) => update(l.key, { credit: v, debit: v ? null : l.debit })} />
                <input className="input" value={l.memo} onChange={(e) => update(l.key, { memo: e.target.value })} placeholder="Optional" />
                <button
                  className="btn btn-ghost btn-icon"
                  aria-label="Remove line"
                  disabled={lines.length <= 2}
                  onClick={() => setLines((ls) => ls.filter((x) => x.key !== l.key))}
                >
                  <Trash2 size={15} />
                </button>
              </div>
            ))}
            <div>
              <button className="btn btn-ghost btn-sm" onClick={() => setLines((ls) => [...ls, blank()])}>
                <Plus size={14} /> Add line
              </button>
            </div>
            <div className="je-grid je-totals">
              <span>Totals</span>
              <span className="right num">{money(debits)}</span>
              <span className="right num">{money(credits)}</span>
              <span />
              <span />
            </div>
            {offBy !== 0 && (
              <div className="banner warn">
                Out of balance by {money(Math.abs(offBy))} — {offBy > 0 ? "add credits" : "add debits"} until both columns match.
              </div>
            )}
            {offBy === 0 && debits > 0 && !journalReady && (
              <div className="banner warn">Every line needs an account and an amount (and at least two lines).</div>
            )}
          </div>
        )}
      </div>
    </Modal>
  );
}
