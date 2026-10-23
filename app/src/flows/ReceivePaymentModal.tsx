// Record money a customer paid you and apply it to their open invoices.

import { CheckCircle2 } from "lucide-react";
import { useMemo, useState } from "react";
import { useAction, useApi } from "../api";
import { AccountSelect, BigMoney, ContactSelect, Field, Loading, Modal, MoneyInput, StatusPill, useToast } from "../components/ui";
import { date, daysBetween, money, plural, todayISO } from "../format";
import type { Account, Cents, InvoiceSummary, NewPayment } from "../types";
import "../pages/Invoice.css";

const METHODS = ["ACH", "Check", "Card", "Cash", "Other"];
const UNPAID = new Set(["open", "partial", "overdue"]);

export default function ReceivePaymentModal(props: { open: boolean; onClose: () => void; customerId?: number; invoiceId?: number }) {
  if (!props.open) return null;
  return <ReceivePayment {...props} />;
}

function ReceivePayment(props: { onClose: () => void; customerId?: number; invoiceId?: number }) {
  const invoices = useApi<InvoiceSummary[]>("invoices.list");
  const accounts = useApi<Account[]>("accounts.list");

  if (invoices.isLoading || accounts.isLoading) {
    return (
      <Modal open onClose={props.onClose} title="Receive payment">
        <Loading />
      </Modal>
    );
  }
  const all = invoices.data ?? [];
  const target = props.invoiceId != null ? all.find((i) => i.id === props.invoiceId) : undefined;
  const firstBank = (accounts.data ?? []).find((a) => a.subtype === "bank" && !a.archived);
  return (
    <PaymentForm
      onClose={props.onClose}
      invoices={all}
      initialCustomer={target?.customer_id ?? props.customerId ?? null}
      initialInvoice={target && target.balance > 0 ? target : undefined}
      defaultAccount={firstBank?.id ?? null}
    />
  );
}

function PaymentForm(props: {
  onClose: () => void;
  invoices: InvoiceSummary[];
  initialCustomer: number | null;
  initialInvoice?: InvoiceSummary;
  defaultAccount: number | null;
}) {
  const toast = useToast();
  const receive = useAction<number, NewPayment>("payments.receive");
  const today = todayISO();

  const [customerId, setCustomerId] = useState<number | null>(props.initialCustomer);
  const [applied, setApplied] = useState<Record<number, Cents | null>>(() =>
    props.initialInvoice ? { [props.initialInvoice.id]: props.initialInvoice.balance } : {},
  );
  const [day, setDay] = useState(today);
  const [accountId, setAccountId] = useState<number | null>(props.defaultAccount);
  const [method, setMethod] = useState("ACH");
  const [reference, setReference] = useState("");
  const [memo, setMemo] = useState("");
  const [error, setError] = useState<string | null>(null);

  const open = useMemo(
    () =>
      props.invoices
        .filter((i) => i.customer_id === customerId && UNPAID.has(i.status) && i.balance > 0)
        .sort((a, b) => a.due_date.localeCompare(b.due_date)),
    [props.invoices, customerId],
  );

  const checked = open.filter((i) => i.id in applied);
  const total = checked.reduce((s, i) => s + (applied[i.id] ?? 0), 0);
  const owed = open.reduce((s, i) => s + i.balance, 0);
  const overApplied = checked.filter((i) => (applied[i.id] ?? 0) > i.balance);

  const toggle = (inv: InvoiceSummary) =>
    setApplied((a) => {
      const next = { ...a };
      if (inv.id in next) delete next[inv.id];
      else next[inv.id] = inv.balance;
      return next;
    });

  const allChecked = open.length > 0 && checked.length === open.length;
  const toggleAll = () => setApplied(allChecked ? {} : Object.fromEntries(open.map((i) => [i.id, i.balance])));

  const submit = async () => {
    setError(null);
    if (customerId == null) return setError("Choose who paid you.");
    if (accountId == null) return setError("Choose where the money was deposited.");
    const applications = checked
      .filter((i) => (applied[i.id] ?? 0) > 0)
      .map((i) => ({ document_id: i.id, amount: applied[i.id]! }));
    if (applications.length === 0) return setError("Check at least one invoice and enter an amount.");
    try {
      await receive.mutateAsync({
        contact_id: customerId,
        date: day,
        account_id: accountId,
        method,
        reference: reference.trim(),
        memo: memo.trim(),
        applications,
      });
      toast({ text: `${money(total)} recorded. We'll match it automatically when the deposit shows up in your bank feed.` });
      props.onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <Modal
      open
      onClose={props.onClose}
      title="Receive payment"
      subtitle="Record money a customer paid you and mark their invoices paid."
      width={640}
      footer={
        <>
          <button className="btn btn-secondary" onClick={props.onClose}>
            Cancel
          </button>
          <button className="btn btn-primary" onClick={submit} disabled={receive.isPending || total <= 0}>
            <CheckCircle2 size={16} /> {receive.isPending ? "Saving…" : total > 0 ? `Record ${money(total)}` : "Record payment"}
          </button>
        </>
      }
    >
      <div className="stack">
        {error && <div className="banner error">{error}</div>}

        <div className="pay-total">
          <div className="grow">
            <Field label="Customer">
              <ContactSelect
                kind="customer"
                value={customerId}
                onChange={(id) => {
                  setCustomerId(id);
                  setApplied({});
                }}
              />
            </Field>
          </div>
          <div className="right">
            <div className="label">Amount received</div>
            <div className="pay-total-amount">
              <BigMoney cents={total} />
            </div>
          </div>
        </div>

        {customerId != null && (
          <div className="pay-invoices">
            {open.length === 0 ? (
              <div className="empty" style={{ padding: "22px 16px" }}>
                <div className="empty-title" style={{ fontSize: 15 }}>Nothing to collect</div>
                <div className="subtle small" style={{ marginTop: 4 }}>This customer has no unpaid invoices.</div>
              </div>
            ) : (
              <>
                <div className="pay-invoices-head">
                  <label className="row" style={{ cursor: "pointer" }}>
                    <input type="checkbox" className="checkbox" checked={allChecked} onChange={toggleAll} />
                    <span className="label">{plural(open.length, "open invoice")}</span>
                  </label>
                  <span className="subtle small">
                    Owed <span className="num bold muted">{money(owed)}</span>
                  </span>
                </div>
                {open.map((inv) => {
                  const on = inv.id in applied;
                  const late = daysBetween(inv.due_date, today);
                  const over = on && (applied[inv.id] ?? 0) > inv.balance;
                  return (
                    <div key={inv.id} className={`pay-invoice ${on ? "on" : ""}`}>
                      <input
                        type="checkbox"
                        className="checkbox"
                        id={`apply-invoice-${inv.id}`}
                        checked={on}
                        onChange={() => toggle(inv)}
                        aria-label={`Apply to invoice ${inv.number}`}
                      />
                      {/* A real label: clicking the text toggles the checkbox, for every kind of input. */}
                      <label className="grow" htmlFor={`apply-invoice-${inv.id}`} style={{ cursor: "pointer" }}>
                        <div className="row">
                          <span className="bold">Invoice #{inv.number}</span>
                          <StatusPill status={inv.status} />
                        </div>
                        <div className={`small ${late > 0 ? "neg" : "subtle"}`}>
                          {late > 0 ? `${plural(late, "day")} late` : `Due ${date(inv.due_date)}`} · balance {money(inv.balance)}
                        </div>
                      </label>
                      <div style={{ width: 140 }}>
                        <MoneyInput
                          value={on ? (applied[inv.id] ?? null) : null}
                          placeholder={on ? "0.00" : "—"}
                          className={over ? "pay-over" : ""}
                          onChange={(c) => setApplied((a) => ({ ...a, [inv.id]: c }))}
                        />
                      </div>
                    </div>
                  );
                })}
                {overApplied.length > 0 && (
                  <div className="banner warn small" style={{ margin: 12 }}>
                    That's more than invoice #{overApplied[0].number} owes ({money(overApplied[0].balance)}).
                  </div>
                )}
              </>
            )}
          </div>
        )}

        <div className="form-grid">
          <Field label="Date received">
            <input className="input" type="date" value={day} onChange={(e) => setDay(e.target.value)} />
          </Field>
          <Field label="Deposit to" hint="Usually your business checking account.">
            <AccountSelect value={accountId} onChange={setAccountId} subtypes={["bank", "other_current_asset"]} placeholder="Choose an account" />
          </Field>
          <Field label="Payment method">
            <select className="select" value={method} onChange={(e) => setMethod(e.target.value)}>
              {METHODS.map((m) => (
                <option key={m}>{m}</option>
              ))}
            </select>
          </Field>
          <Field label="Reference" hint="Check number or confirmation code">
            <input className="input" value={reference} onChange={(e) => setReference(e.target.value)} placeholder="Optional" />
          </Field>
          <Field label="Memo" className="span-2">
            <input className="input" value={memo} onChange={(e) => setMemo(e.target.value)} placeholder="Optional note for your records" />
          </Field>
        </div>
      </div>
    </Modal>
  );
}
