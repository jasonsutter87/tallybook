import { Plus, Send, Trash2, UserPlus } from "lucide-react";
import { useMemo, useState } from "react";
import { Navigate, useNavigate, useParams } from "react-router-dom";
import { useAction, useApi } from "../api";
import { AccountSelect, ContactSelect, ErrorBanner, Field, Loading, MoneyInput, PageHeader, useToast } from "../components/ui";
import QuickContactModal from "../flows/QuickContactModal";
import { addDays, daysBetween, money, parseQty, qty, todayISO } from "../format";
import type { Account, Cents, InvoiceDetail, InvoiceDraft, Settings } from "../types";
import "./Invoice.css";

// ------------------------------------------------------------- math
// Mirrors crates/tally-core/src/invoice.rs exactly: integer math, one
// rounding step, half away from zero.

function divRound(n: bigint, d: bigint): bigint {
  const q = n / d;
  const r = n % d;
  const abs = r < 0n ? -r : r;
  if (2n * abs >= d) return q + (n > 0n ? 1n : n < 0n ? -1n : 0n);
  return q;
}

function lineAmount(quantityMilli: number, unitPrice: Cents): Cents {
  return Number(divRound(BigInt(quantityMilli) * BigInt(unitPrice), 1000n));
}

function taxOn(taxable: Cents, rateBp: number): Cents {
  return Number(divRound(BigInt(taxable) * BigInt(rateBp), 10_000n));
}

/** "8.25" → 825 basis points; null if not a valid percentage. */
function parseRate(s: string): number | null {
  const t = s.trim().replace(/%$/, "");
  if (t === "") return 0;
  if (!/^\d*\.?\d{0,2}$/.test(t) || t === ".") return null;
  return Math.round(Number(t) * 100);
}

function rateToInput(bp: number): string {
  return bp ? String(bp / 100) : "";
}

// ------------------------------------------------------------- types

interface Row {
  key: number;
  description: string;
  qtyText: string;
  price: Cents | null;
  accountId: number | null;
  taxable: boolean;
}

const TERMS = [
  { days: 0, label: "Due on receipt" },
  { days: 15, label: "Net 15" },
  { days: 30, label: "Net 30" },
  { days: 60, label: "Net 60" },
];

let nextKey = 1;

// ------------------------------------------------------------- loader

export default function InvoiceEditor() {
  const { id } = useParams();
  const editId = id ? Number(id) : null;
  const settings = useApi<Settings>("settings.get");
  const accounts = useApi<Account[]>("accounts.list");
  const invoice = useApi<InvoiceDetail>("invoices.get", { id: editId }, { enabled: editId != null });

  if (settings.isLoading || accounts.isLoading || (editId != null && invoice.isLoading)) return <Loading />;
  const err = settings.error ?? accounts.error ?? invoice.error;
  if (err)
    return (
      <div className="page page-narrow">
        <PageHeader title="Invoice" back={{ to: "/invoices", label: "Invoices" }} />
        <ErrorBanner error={err} />
      </div>
    );
  if (editId != null && invoice.data && invoice.data.status !== "draft") return <Navigate to={`/invoices/${editId}`} replace />;

  const income = (accounts.data ?? []).filter((a) => a.kind === "income" && !a.archived);
  const defaultAccount = (income.find((a) => a.code === "4100") ?? income.find((a) => a.subtype === "income") ?? income[0])?.id ?? null;

  return (
    <Editor
      key={editId ?? "new"}
      prefs={settings.data?.prefs ?? {}}
      existing={editId != null ? invoice.data : undefined}
      defaultAccount={defaultAccount}
    />
  );
}

// ------------------------------------------------------------- editor

function Editor(props: { prefs: Record<string, string>; existing?: InvoiceDetail; defaultAccount: number | null }) {
  const navigate = useNavigate();
  const toast = useToast();
  const { prefs, existing, defaultAccount } = props;

  const create = useAction<number, InvoiceDraft>("invoices.create");
  const update = useAction<null, { id: number; draft: InvoiceDraft }>("invoices.update");
  const finalize = useAction<number, { id: number }>("invoices.finalize");

  const prefTerms = Number(prefs["invoice.default_terms_days"]);
  const defaultTerms = Number.isFinite(prefTerms) && prefs["invoice.default_terms_days"] ? prefTerms : 30;

  const blankRow = (): Row => ({ key: nextKey++, description: "", qtyText: "1", price: null, accountId: defaultAccount, taxable: false });

  const [customerId, setCustomerId] = useState<number | null>(existing?.customer_id ?? null);
  const [issueDate, setIssueDate] = useState(existing?.issue_date ?? todayISO());
  const [terms, setTerms] = useState<number | "custom">(() => {
    if (!existing) return defaultTerms;
    const d = daysBetween(existing.issue_date, existing.due_date);
    return TERMS.some((t) => t.days === d) ? d : "custom";
  });
  const [dueDate, setDueDate] = useState(existing?.due_date ?? addDays(todayISO(), defaultTerms));
  const [rows, setRows] = useState<Row[]>(() =>
    existing
      ? existing.lines.map((l) => ({
          key: nextKey++,
          description: l.description,
          qtyText: qty(l.quantity_milli),
          price: l.unit_price,
          accountId: l.account_id,
          taxable: l.taxable,
        }))
      : [blankRow()],
  );
  const [taxText, setTaxText] = useState(() =>
    rateToInput(existing ? existing.tax_rate_bp : Number(prefs["invoice.default_tax_bp"]) || 0),
  );
  const [memo, setMemo] = useState(existing?.memo ?? prefs["invoice.footer"] ?? "Thanks for your business!");
  const [error, setError] = useState<string | null>(null);
  const [newCustomer, setNewCustomer] = useState(false);
  const [busy, setBusy] = useState<"draft" | "issue" | null>(null);

  const rateBp = parseRate(taxText);

  const computed = useMemo(() => {
    let subtotal = 0;
    let taxable = 0;
    const amounts = rows.map((r) => {
      const q = parseQty(r.qtyText);
      if (q == null || r.price == null) return null;
      const a = lineAmount(q, r.price);
      subtotal += a;
      if (r.taxable) taxable += a;
      return a;
    });
    const tax = rateBp != null ? taxOn(taxable, rateBp) : 0;
    return { amounts, subtotal, taxable, tax, total: subtotal + tax };
  }, [rows, rateBp]);

  const anyTaxable = rows.some((r) => r.taxable);

  const setRow = (key: number, patch: Partial<Row>) => setRows((rs) => rs.map((r) => (r.key === key ? { ...r, ...patch } : r)));

  const changeIssueDate = (d: string) => {
    setIssueDate(d);
    if (terms !== "custom" && d) setDueDate(addDays(d, terms));
  };

  const changeTerms = (v: string) => {
    if (v === "custom") return setTerms("custom");
    const days = Number(v);
    setTerms(days);
    if (issueDate) setDueDate(addDays(issueDate, days));
  };

  const changeDueDate = (d: string) => {
    setDueDate(d);
    if (!d || !issueDate) return;
    const days = daysBetween(issueDate, d);
    setTerms(TERMS.some((t) => t.days === days) ? days : "custom");
  };

  /** Builds the draft, or reports the first problem in plain English. */
  const buildDraft = (): InvoiceDraft | string => {
    if (customerId == null) return "Choose a customer to bill.";
    if (!issueDate) return "Pick an invoice date.";
    if (!dueDate) return "Pick a due date.";
    if (dueDate < issueDate) return "The due date can't be before the invoice date.";
    if (rateBp == null) return "Tax rate should be a percentage like 8.25.";
    const filled = rows.filter((r) => r.description.trim() || r.price != null);
    if (filled.length === 0) return "Add at least one line item.";
    const lines: InvoiceDraft["lines"] = [];
    for (const [i, r] of filled.entries()) {
      const q = parseQty(r.qtyText);
      if (q == null || q <= 0) return `Line ${i + 1}: quantity should be a number above zero.`;
      if (r.price == null) return `Line ${i + 1}: enter a rate.`;
      if (r.accountId == null) return `Line ${i + 1}: choose an income category.`;
      lines.push({ description: r.description.trim(), quantity_milli: q, unit_price: r.price, account_id: r.accountId, taxable: r.taxable });
    }
    return { customer_id: customerId, issue_date: issueDate, due_date: dueDate, memo: memo.trim(), tax_rate_bp: rateBp, lines };
  };

  const save = async (issue: boolean) => {
    const draft = buildDraft();
    if (typeof draft === "string") {
      setError(draft);
      return;
    }
    setError(null);
    setBusy(issue ? "issue" : "draft");
    let id = existing?.id ?? null;
    try {
      if (id != null) await update.mutateAsync({ id, draft });
      else id = await create.mutateAsync(draft);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(null);
      return;
    }
    if (issue) {
      try {
        await finalize.mutateAsync({ id });
        toast({ text: "Invoice issued. It's now in your receivables." });
      } catch (e) {
        toast.error(`Saved as a draft, but couldn't issue it: ${e instanceof Error ? e.message : String(e)}`);
      }
    } else {
      toast({ text: "Draft saved" });
    }
    setBusy(null);
    navigate(`/invoices/${id}`, { replace: true });
  };

  const numberLabel = existing ? `#${existing.number}` : prefs["invoice.next_number"] ? `#${prefs["invoice.next_number"]}` : "New";

  const actions = (
    <>
      <button className="btn btn-ghost" onClick={() => navigate(existing ? `/invoices/${existing.id}` : "/invoices")} disabled={busy != null}>
        Cancel
      </button>
      <button className="btn btn-secondary" onClick={() => save(false)} disabled={busy != null}>
        {busy === "draft" ? "Saving…" : "Save draft"}
      </button>
      <button className="btn btn-primary" onClick={() => save(true)} disabled={busy != null}>
        <Send size={15} /> {busy === "issue" ? "Issuing…" : "Save & issue"}
      </button>
    </>
  );

  return (
    <div className="page">
      <PageHeader
        title={existing ? `Edit invoice #${existing.number}` : "New invoice"}
        subtitle={existing ? "This is still a draft. Nothing hits your books until you issue it." : "Drafts stay private until you issue them."}
        back={{ to: existing ? `/invoices/${existing.id}` : "/invoices", label: existing ? "Back to invoice" : "Invoices" }}
        actions={actions}
      />

      {error && (
        <div className="banner error" style={{ marginBottom: 16 }}>
          {error}
        </div>
      )}

      <section className="card inv-sheet">
        <div className="inv-sheet-head">
          <div>
            <div className="inv-sheet-title">Invoice</div>
            <div className="inv-sheet-number num">{numberLabel}</div>
            <div className="field">
              <div className="row-between">
                <span className="label">Bill to</span>
                <button type="button" className="btn btn-ghost btn-sm" style={{ height: 22, padding: "0 6px" }} onClick={() => setNewCustomer(true)}>
                  <UserPlus size={13} /> New customer
                </button>
              </div>
              <ContactSelect kind="customer" value={customerId} onChange={setCustomerId} />
            </div>
          </div>
          <div className="form-grid" style={{ alignContent: "end" }}>
            <Field label="Invoice date">
              <input className="input" type="date" value={issueDate} onChange={(e) => changeIssueDate(e.target.value)} />
            </Field>
            <Field label="Terms">
              <select className="select" value={String(terms)} onChange={(e) => changeTerms(e.target.value)}>
                {TERMS.map((t) => (
                  <option key={t.days} value={t.days}>
                    {t.label}
                  </option>
                ))}
                <option value="custom">Custom</option>
              </select>
            </Field>
            <Field label="Due date" className="span-2">
              <input className="input" type="date" value={dueDate} min={issueDate} onChange={(e) => changeDueDate(e.target.value)} />
            </Field>
          </div>
        </div>

        <table className="inv-lines">
          <thead>
            <tr>
              <th style={{ width: "34%" }}>Description</th>
              <th className="right" style={{ width: 84 }}>
                Qty
              </th>
              <th className="right" style={{ width: 130 }}>
                Rate
              </th>
              <th style={{ width: "22%" }}>Category</th>
              <th className="center" style={{ width: 54 }}>
                Tax
              </th>
              <th className="right" style={{ width: 120 }}>
                Amount
              </th>
              <th style={{ width: 40 }} />
            </tr>
          </thead>
          <tbody>
            {rows.map((r, i) => {
              const qtyBad = r.qtyText.trim() !== "" && (parseQty(r.qtyText) == null || parseQty(r.qtyText) === 0);
              const amount = computed.amounts[i];
              return (
                <tr key={r.key}>
                  <td>
                    <input
                      className="input"
                      aria-label="Description"
                      placeholder="What did you do or sell?"
                      value={r.description}
                      onChange={(e) => setRow(r.key, { description: e.target.value })}
                    />
                  </td>
                  <td>
                    <input
                      className="input num"
                      inputMode="decimal"
                      aria-label="Quantity"
                      value={r.qtyText}
                      style={qtyBad ? { borderColor: "var(--neg)" } : undefined}
                      onChange={(e) => setRow(r.key, { qtyText: e.target.value })}
                    />
                  </td>
                  <td>
                    <MoneyInput value={r.price} onChange={(c) => setRow(r.key, { price: c })} aria-label="Price" />
                  </td>
                  <td>
                    <AccountSelect kinds={["income"]} value={r.accountId} onChange={(a) => setRow(r.key, { accountId: a })} placeholder="Income category" />
                  </td>
                  <td className="center">
                    <input
                      type="checkbox"
                      className="checkbox"
                      checked={r.taxable}
                      onChange={(e) => setRow(r.key, { taxable: e.target.checked })}
                      aria-label="Taxable"
                    />
                  </td>
                  <td className="right num inv-line-amount">{amount == null ? <span className="subtle">—</span> : money(amount)}</td>
                  <td>
                    <button
                      type="button"
                      className="btn btn-ghost btn-icon inv-remove"
                      aria-label="Remove line"
                      disabled={rows.length === 1}
                      onClick={() => setRows((rs) => rs.filter((x) => x.key !== r.key))}
                    >
                      <Trash2 size={15} />
                    </button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
        <button
          type="button"
          className="btn btn-ghost btn-sm"
          style={{ marginTop: 8 }}
          onClick={() => {
            const last = rows[rows.length - 1];
            setRows((rs) => [...rs, { ...blankRow(), accountId: last?.accountId ?? defaultAccount, taxable: last?.taxable ?? false }]);
          }}
        >
          <Plus size={15} /> Add line
        </button>

        <div className="inv-sheet-foot">
          <Field label="Message on invoice" hint="Shown at the bottom of the invoice your customer sees.">
            <textarea className="textarea" rows={4} value={memo} onChange={(e) => setMemo(e.target.value)} placeholder="Thanks for your business!" />
          </Field>
          <div className="inv-totals">
            <div className="inv-totals-row">
              <span>Subtotal</span>
              <span className="num">{money(computed.subtotal)}</span>
            </div>
            <div className="inv-totals-row">
              <span className="row" style={{ gap: 8 }}>
                Tax
                <input
                  className="input num inv-tax-input"
                  inputMode="decimal"
                  placeholder="0"
                  value={taxText}
                  style={rateBp == null ? { borderColor: "var(--neg)" } : undefined}
                  onChange={(e) => setTaxText(e.target.value)}
                  aria-label="Tax rate percent"
                />
                <span className="subtle">%</span>
              </span>
              <span className="num">{money(computed.tax)}</span>
            </div>
            {rateBp != null && rateBp > 0 && !anyTaxable && <div className="hint">Check "Tax" on the lines that are taxable.</div>}
            <div className="inv-totals-row total">
              <span>Total</span>
              <span className="num">{money(computed.total)}</span>
            </div>
          </div>
        </div>
      </section>

      <div className="inv-actions-bar">{actions}</div>

      <QuickContactModal open={newCustomer} onClose={() => setNewCustomer(false)} kind="customer" onCreated={setCustomerId} />
    </div>
  );
}
