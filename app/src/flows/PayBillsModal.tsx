// Pay one or more of a vendor's open bills in a single payment.

import { CheckCircle2, Receipt } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useAction, useApi } from "../api";
import { AccountSelect, BigMoney, ContactSelect, ErrorBanner, Field, Modal, MoneyInput, useToast } from "../components/ui";
import { date, money, plural } from "../format";
import { DueHint, PAY_METHODS, isUnpaid, useToday } from "../pages/billsShared";
import type { Account, BillSummary, Cents, NewPayment } from "../types";
import "../pages/bills.css";

type Pick = { checked: boolean; amount: Cents | null };

export default function PayBillsModal(props: { open: boolean; onClose: () => void; vendorId?: number; billId?: number }) {
  const toast = useToast();
  const today = useToday();
  const bills = useApi<BillSummary[]>("bills.list", undefined, { enabled: props.open });
  const accounts = useApi<Account[]>("accounts.list", undefined, { enabled: props.open });
  const pay = useAction<number, NewPayment>("payments.pay");

  const [vendorId, setVendorId] = useState<number | null>(null);
  const [picks, setPicks] = useState<Record<number, Pick>>({});
  const [accountId, setAccountId] = useState<number | null>(null);
  const [payDate, setPayDate] = useState(today);
  const [method, setMethod] = useState<string>("ACH");
  const [reference, setReference] = useState("");
  const [memo, setMemo] = useState("");
  const [error, setError] = useState<Error | null>(null);
  const [ready, setReady] = useState(false);

  // Reset when closed so the next open starts fresh.
  useEffect(() => {
    if (!props.open) setReady(false);
  }, [props.open]);

  // Initialize once the data we need has loaded.
  useEffect(() => {
    if (!props.open || ready || !bills.data || !accounts.data) return;
    const target = props.billId != null ? bills.data.find((b) => b.id === props.billId) : undefined;
    const owed = owedVendors(bills.data);
    const vid = target?.vendor_id ?? props.vendorId ?? (owed.length === 1 ? owed[0].id : null);
    setVendorId(vid);
    setPicks(initialPicks(bills.data, vid, props.billId));
    const bank = accounts.data.find((a) => !a.archived && a.subtype === "bank");
    setAccountId(bank?.id ?? null);
    setPayDate(today);
    setMethod("ACH");
    setReference("");
    setMemo("");
    setError(null);
    setReady(true);
  }, [props.open, ready, bills.data, accounts.data, props.billId, props.vendorId, today]);

  const open = useMemo(
    () =>
      (bills.data ?? [])
        .filter((b) => b.vendor_id === vendorId && isUnpaid(b))
        .sort((a, b) => a.due_date.localeCompare(b.due_date)),
    [bills.data, vendorId],
  );

  const chosen = open.filter((b) => picks[b.id]?.checked);
  const total = chosen.reduce((s, b) => s + (picks[b.id]?.amount ?? 0), 0);
  const vendorName = open[0]?.vendor_name;

  const changeVendor = (id: number | null) => {
    setVendorId(id);
    setPicks(initialPicks(bills.data ?? [], id));
    setError(null);
  };

  const setPick = (id: number, p: Partial<Pick>) => setPicks((ps) => ({ ...ps, [id]: { ...ps[id], ...p } }));

  const submit = async () => {
    setError(null);
    if (!vendorId) return setError(new Error("Choose who you're paying."));
    if (!chosen.length) return setError(new Error("Pick at least one bill to pay."));
    for (const b of chosen) {
      const amt = picks[b.id]?.amount ?? 0;
      if (amt <= 0) return setError(new Error(`Enter an amount for bill ${b.number || `#${b.id}`}.`));
      if (amt > b.balance) return setError(new Error(`Bill ${b.number || `#${b.id}`} only has ${money(b.balance)} left to pay.`));
    }
    if (!accountId) return setError(new Error("Choose the account the money comes out of."));
    try {
      await pay.mutateAsync({
        contact_id: vendorId,
        date: payDate,
        account_id: accountId,
        method,
        reference: reference.trim(),
        memo: memo.trim(),
        applications: chosen.map((b) => ({ document_id: b.id, amount: picks[b.id]!.amount! })),
      });
      toast({ text: `Paid ${money(total)}${vendorName ? ` to ${vendorName}` : ""}` });
      props.onClose();
    } catch (e) {
      setError(e as Error);
    }
  };

  return (
    <Modal
      open={props.open}
      onClose={props.onClose}
      width={640}
      title="Pay bills"
      subtitle="Record money you've sent to a vendor."
      footer={
        <>
          <button className="btn btn-secondary" onClick={props.onClose}>
            Cancel
          </button>
          <button className="btn btn-primary" onClick={submit} disabled={pay.isPending || !ready || total <= 0}>
            {pay.isPending ? "Saving…" : total > 0 ? `Pay ${money(total)}` : "Pay"}
          </button>
        </>
      }
    >
      <div className="stack" style={{ gap: 18 }}>
        <ErrorBanner error={error ?? bills.error ?? accounts.error} />

        <Field label="Vendor">
          <ContactSelect kind="vendor" value={vendorId} onChange={changeVendor} />
        </Field>

        {vendorId == null && owedVendors(bills.data ?? []).length > 0 && (
          <div>
            <div className="label" style={{ marginBottom: 8 }}>
              Vendors you owe
            </div>
            <div className="row" style={{ flexWrap: "wrap", gap: 8 }}>
              {owedVendors(bills.data ?? []).map((v) => (
                <button key={v.id} type="button" className="btn btn-secondary btn-sm" onClick={() => changeVendor(v.id)}>
                  {v.name} · {money(v.balance)}
                </button>
              ))}
            </div>
          </div>
        )}

        {vendorId != null && (
          <div className="card" style={{ boxShadow: "none" }}>
            {open.length === 0 ? (
              <div className="row" style={{ padding: "18px 20px", gap: 12 }}>
                <span className="icon-bubble pos">
                  <CheckCircle2 size={18} />
                </span>
                <div>
                  <div className="bold">All caught up</div>
                  <div className="muted small">This vendor has no unpaid bills.</div>
                </div>
              </div>
            ) : (
              <>
                <div className="row-between" style={{ padding: "12px 20px 4px" }}>
                  <span className="label">{plural(open.length, "unpaid bill")}</span>
                  <button
                    className="btn btn-ghost btn-sm"
                    onClick={() => {
                      const all = open.every((b) => picks[b.id]?.checked);
                      setPicks((ps) => {
                        const next = { ...ps };
                        for (const b of open) next[b.id] = { checked: !all, amount: ps[b.id]?.amount ?? b.balance };
                        return next;
                      });
                    }}
                  >
                    {open.every((b) => picks[b.id]?.checked) ? "Clear all" : "Select all"}
                  </button>
                </div>
                {open.map((b) => {
                  const p = picks[b.id] ?? { checked: false, amount: b.balance };
                  return (
                    <div key={b.id} className="list-row" style={{ padding: "10px 20px", opacity: p.checked ? 1 : 0.75 }}>
                      <input
                        type="checkbox"
                        className="checkbox"
                        checked={p.checked}
                        onChange={(e) => setPick(b.id, { checked: e.target.checked, amount: p.amount ?? b.balance })}
                        aria-label={`Pay bill ${b.number}`}
                      />
                      <div className="grow" style={{ minWidth: 0 }}>
                        <div className="bold truncate">
                          <Receipt size={13} style={{ verticalAlign: -2, marginRight: 6, color: "var(--ink-3)" }} />
                          {b.number ? `Bill ${b.number}` : `Bill from ${date(b.issue_date)}`}
                        </div>
                        <div className="small muted">
                          Due {date(b.due_date)} · {money(b.balance)} left
                          {b.paid > 0 && <> of {money(b.total)}</>}
                        </div>
                        <DueHint bill={b} today={today} />
                      </div>
                      <div style={{ width: 140 }}>
                        <MoneyInput
                          value={p.checked ? p.amount : null}
                          placeholder={money(b.balance).replace("$", "")}
                          onChange={(c) => setPick(b.id, { amount: c, checked: c != null && c > 0 ? true : p.checked })}
                        />
                      </div>
                    </div>
                  );
                })}
              </>
            )}
          </div>
        )}

        <div className="form-grid">
          <Field label="Pay from">
            <AccountSelect value={accountId} onChange={setAccountId} subtypes={["bank", "credit_card", "other_current_asset"]} placeholder="Choose an account" />
          </Field>
          <Field label="Payment date">
            <input className="input" type="date" value={payDate} onChange={(e) => setPayDate(e.target.value)} />
          </Field>
          <Field label="Method">
            <select className="select" value={method} onChange={(e) => setMethod(e.target.value)}>
              {PAY_METHODS.map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
            </select>
          </Field>
          <Field label={method === "Check" ? "Check number" : "Reference"} hint="Optional">
            <input className="input" value={reference} onChange={(e) => setReference(e.target.value)} placeholder={method === "Check" ? "e.g. 1042" : "Confirmation #"} />
          </Field>
          <Field label="Memo" className="span-2" hint="Optional">
            <input className="input" value={memo} onChange={(e) => setMemo(e.target.value)} />
          </Field>
        </div>

        <div
          className="row-between"
          style={{ background: "var(--surface-2)", border: "1px solid var(--line)", borderRadius: 18, padding: "16px 20px" }}
        >
          <div>
            <div className="label">Total payment</div>
            <div className="small subtle">{chosen.length ? plural(chosen.length, "bill") : "No bills selected"}</div>
          </div>
          <div style={{ fontSize: 30 }}>
            <BigMoney cents={total} className="bk-big" />
          </div>
        </div>
      </div>
    </Modal>
  );
}

function initialPicks(bills: BillSummary[], vendorId: number | null, onlyBill?: number): Record<number, Pick> {
  const out: Record<number, Pick> = {};
  for (const b of bills) {
    if (b.vendor_id !== vendorId || !isUnpaid(b)) continue;
    out[b.id] = { checked: onlyBill == null || b.id === onlyBill, amount: b.balance };
  }
  return out;
}

function owedVendors(bills: BillSummary[]): { id: number; name: string; balance: Cents }[] {
  const m = new Map<number, { id: number; name: string; balance: Cents }>();
  for (const b of bills) {
    if (!isUnpaid(b)) continue;
    const v = m.get(b.vendor_id) ?? { id: b.vendor_id, name: b.vendor_name, balance: 0 };
    v.balance += b.balance;
    m.set(b.vendor_id, v);
  }
  return [...m.values()].sort((a, b) => b.balance - a.balance);
}
