// Add or change a household bill that comes around again. Once saved, the
// backend ticks each due date off when a matching payment arrives.

import { useEffect, useState } from "react";
import { call, queryClient } from "../api";
import { PersonSelect } from "../components/household";
import { AccountSelect, ErrorBanner, Field, Modal, MoneyInput, useToast } from "../components/ui";
import { todayISO } from "../format";
import type { Cadence, Cents, NewRecurringBill, RecurringBill } from "../types";

type Every = Cadence["every"];

// The same limits the backend checks (Cadence::validate in crates/tally-core/src/schedule.rs).
const LAST_DUE_DAY = 31;
const MOST_MONTHS_APART = 24;
/** Where "every few months" starts: twice a year. */
const DEFAULT_MONTHS_APART = 6;

/** Clamp a typed number into 1..max, treating junk as 1. */
const clampInput = (raw: string, max: number) => Math.min(max, Math.max(1, Number(raw) || 1));

/**
 * A bill to open the form with: a saved bill (has `id`, so saving updates it)
 * or a partly filled new one, such as a suggestion.
 */
export interface BillDraft extends Partial<NewRecurringBill> {
  id?: number;
}

export default function RecurringBillModal(props: { open: boolean; onClose: () => void; bill: BillDraft | RecurringBill | null }) {
  const toast = useToast();
  const [name, setName] = useState("");
  const [amount, setAmount] = useState<Cents | null>(null);
  const [varies, setVaries] = useState(false);
  const [category, setCategory] = useState<number | null>(null);
  const [paidFrom, setPaidFrom] = useState<number | null>(null);
  const [person, setPerson] = useState<number | null>(null);
  const [every, setEvery] = useState<Every>("month");
  const [day, setDay] = useState(1);
  const [months, setMonths] = useState(DEFAULT_MONTHS_APART);
  const [start, setStart] = useState(todayISO());
  const [autoPay, setAutoPay] = useState(false);
  const [matchText, setMatchText] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<Error | null>(null);

  useEffect(() => {
    if (!props.open) return;
    const b = props.bill ?? {};
    setName(b.name ?? "");
    setAmount(b.amount || null);
    setVaries(b.varies ?? false);
    setCategory(b.account_id ?? null);
    setPaidFrom(b.paid_from_id ?? null);
    setPerson(b.person_id ?? null);
    const c = b.cadence;
    setEvery(c?.every ?? "month");
    setDay(c && c.every !== "week" ? c.day : 1);
    setMonths(c?.every === "months" ? c.months : DEFAULT_MONTHS_APART);
    setStart(b.start_date ?? todayISO());
    setAutoPay(b.auto_pay ?? false);
    setMatchText(b.match_text ?? "");
    setError(null);
  }, [props.open, props.bill]);

  const cadence: Cadence = every === "week" ? { every } : every === "months" ? { every, months, day } : { every, day };
  // The id of the bill being changed; null when adding a new one.
  const editingId = props.bill && "id" in props.bill && props.bill.id != null ? props.bill.id : null;
  // A bill that varies needs its typical amount: that's what payments are matched against.
  const ready = !!name.trim() && category != null && (!varies || !!amount);

  const save = async () => {
    if (!ready) return;
    setBusy(true);
    setError(null);
    const bill: NewRecurringBill = {
      name: name.trim(),
      amount: amount ?? 0,
      varies,
      account_id: category!,
      paid_from_id: paidFrom,
      person_id: person,
      cadence,
      start_date: start,
      auto_pay: autoPay,
      match_text: matchText.trim() || null,
    };
    try {
      if (editingId != null) await call("recurring.update", { id: editingId, bill });
      else await call("recurring.create", bill);
      await queryClient.invalidateQueries();
      toast({ text: editingId != null ? `${bill.name} updated` : `${bill.name} added` });
      props.onClose();
    } catch (e) {
      setError(e as Error);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open={props.open}
      onClose={props.onClose}
      title={editingId != null ? "Edit bill" : "Add a bill"}
      subtitle="Tallybook ticks it off when the payment shows up in your bank feed."
      footer={
        <>
          <button className="btn btn-secondary" onClick={props.onClose}>
            Cancel
          </button>
          <button className="btn btn-primary" disabled={!ready || busy} onClick={save}>
            {busy ? "Saving…" : editingId != null ? "Save" : "Add bill"}
          </button>
        </>
      }
    >
      <form
        className="stack"
        onSubmit={(e) => {
          e.preventDefault();
          save();
        }}
      >
        <ErrorBanner error={error} />
        <div className="form-grid">
          <Field label="Name">
            <input className="input" autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. Verizon" />
          </Field>
          <Field label={varies ? "Typical amount" : "Amount"} hint={varies ? "Payments from half to double this count." : "Leave blank if you don't know it yet."}>
            <MoneyInput value={amount} onChange={setAmount} />
          </Field>
        </div>
        <Field label="Category">
          <AccountSelect scope="household" kinds={["expense"]} value={category} onChange={setCategory} placeholder="e.g. Phones" />
        </Field>
        <div className="form-grid">
          <Field label="How often">
            <select className="select" value={every} onChange={(e) => setEvery(e.target.value as Every)}>
              <option value="month">Every month</option>
              <option value="months">Every few months</option>
              <option value="week">Every week</option>
            </select>
          </Field>
          {every === "week" ? (
            <Field label="Starting">
              <input className="input" type="date" value={start} onChange={(e) => setStart(e.target.value)} />
            </Field>
          ) : (
            <Field label="Due on day">
              <input className="input" type="number" min={1} max={LAST_DUE_DAY} value={day} onChange={(e) => setDay(clampInput(e.target.value, LAST_DUE_DAY))} />
            </Field>
          )}
        </div>
        {every === "months" && (
          <div className="form-grid">
            <Field label="Every how many months">
              <input className="input" type="number" min={1} max={MOST_MONTHS_APART} value={months} onChange={(e) => setMonths(clampInput(e.target.value, MOST_MONTHS_APART))} />
            </Field>
            <Field label="First due" hint="Counts from this month.">
              <input className="input" type="date" value={start} onChange={(e) => setStart(e.target.value)} />
            </Field>
          </div>
        )}
        <div className="form-grid">
          <Field label="Paid from (optional)">
            <AccountSelect scope="household" subtypes={["bank", "credit_card"]} value={paidFrom} onChange={setPaidFrom} placeholder="Account" />
          </Field>
          <Field label="Who it's for (optional)">
            <PersonSelect value={person} onChange={setPerson} placeholder="Shared" />
          </Field>
        </div>
        <Field label="Bank description contains (optional)" hint="Helps tell this bill apart from others in the same category, e.g. VERIZON.">
          <input className="input" value={matchText} onChange={(e) => setMatchText(e.target.value)} />
        </Field>
        <label className="switch">
          <input type="checkbox" checked={varies} onChange={(e) => setVaries(e.target.checked)} />
          <span>Changes month to month (power, water)</span>
        </label>
        <label className="switch">
          <input type="checkbox" checked={autoPay} onChange={(e) => setAutoPay(e.target.checked)} />
          <span>Pays itself (auto-pay)</span>
        </label>
        <button type="submit" hidden />
      </form>
    </Modal>
  );
}
