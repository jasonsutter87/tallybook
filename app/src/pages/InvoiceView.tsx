// One invoice as the customer sees it, ready to print or save as PDF, with its
// payments and the actions to send, record a payment, edit a draft or void.

import { Ban, Check, Copy, HandCoins, Mail, Pencil, Printer, Send, Trash2 } from "lucide-react";
import { useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import VoidPaymentButton from "../components/VoidPaymentButton";
import { useAction, useApi } from "../api";
import { Card, ConfirmModal, ErrorBanner, Loading, Money, PageHeader, StatusPill, useToast } from "../components/ui";
import ReceivePaymentModal from "../flows/ReceivePaymentModal";
import { date, daysBetween, money, plural, qty, todayISO } from "../format";
import type { Contact, InvoiceDetail, Settings } from "../types";
import "./Invoice.css";

const UNPAID = new Set(["open", "partial", "overdue"]);

function rateLabel(bp: number): string {
  return `${bp / 100}%`;
}

export default function InvoiceView() {
  const { id } = useParams();
  const invoiceId = Number(id);
  const navigate = useNavigate();
  const toast = useToast();

  const inv = useApi<InvoiceDetail>("invoices.get", { id: invoiceId });
  const settings = useApi<Settings>("settings.get");
  const contact = useApi<Contact>("contacts.get", { id: inv.data?.customer_id }, { enabled: inv.data != null });

  const finalize = useAction<number, { id: number }>("invoices.finalize");
  const remove = useAction<null, { id: number }>("invoices.delete");
  const voidIt = useAction<null, { id: number }>("invoices.void");

  const [paying, setPaying] = useState(false);
  const [confirm, setConfirm] = useState<"delete" | "void" | null>(null);
  const [copied, setCopied] = useState(false);

  if (inv.isLoading || settings.isLoading) return <Loading />;
  if (inv.error || !inv.data)
    return (
      <div className="page page-narrow">
        <PageHeader title="Invoice" back={{ to: "/invoices", label: "Invoices" }} />
        <ErrorBanner error={inv.error ?? new Error("That invoice couldn't be found.")} />
      </div>
    );

  const d = inv.data;
  const prefs = settings.data?.prefs ?? {};
  const company = prefs["company.name"] || "Your business";
  const c = contact.data;
  const unpaid = UNPAID.has(d.status);
  const today = todayISO();
  const late = daysBetween(d.due_date, today);
  const hasTaxable = d.lines.some((l) => l.taxable);

  const summaryText = [
    `Invoice #${d.number} from ${company}`,
    `Total: ${money(d.total)}`,
    d.paid > 0 ? `Paid: ${money(d.paid)}` : null,
    `Balance due: ${money(d.balance)}`,
    `Due: ${date(d.due_date)}`,
  ]
    .filter(Boolean)
    .join("\n");

  const mailto = c?.email
    ? `mailto:${encodeURIComponent(c.email)}?subject=${encodeURIComponent(`Invoice #${d.number} from ${company}`)}&body=${encodeURIComponent(
        `Hi ${c.name},\n\nHere's invoice #${d.number} for ${money(d.total)}` +
          (d.balance > 0 && d.balance !== d.total ? ` (${money(d.balance)} still due)` : "") +
          `, due ${date(d.due_date)}.\n\n` +
          d.lines.map((l) => `• ${l.description || "Item"}: ${money(l.amount)}`).join("\n") +
          `\n\n${d.memo ? d.memo + "\n\n" : ""}Thanks,\n${company}`,
      )}`
    : null;

  const copyText = async () => {
    try {
      await navigator.clipboard.writeText(summaryText);
      setCopied(true);
      setTimeout(() => setCopied(false), 1800);
    } catch {
      toast.error("Couldn't copy to the clipboard.");
    }
  };

  const issue = async () => {
    try {
      await finalize.mutateAsync({ id: d.id });
      toast({ text: "Invoice issued. It's now in your receivables." });
    } catch (e) {
      toast.error(e);
    }
  };

  const doDelete = async () => {
    try {
      await remove.mutateAsync({ id: d.id });
      toast({ text: `Draft #${d.number} deleted` });
      navigate("/invoices", { replace: true });
    } catch (e) {
      toast.error(e);
    }
  };

  const doVoid = async () => {
    try {
      await voidIt.mutateAsync({ id: d.id });
      toast({ text: `Invoice #${d.number} voided` });
    } catch (e) {
      toast.error(e);
    }
  };

  const print = (
    <button className="btn btn-secondary" onClick={() => window.print()}>
      <Printer size={15} /> Print / Save PDF
    </button>
  );

  let actions;
  if (d.status === "draft") {
    actions = (
      <>
        <button className="btn btn-ghost" onClick={() => setConfirm("delete")}>
          <Trash2 size={15} /> Delete
        </button>
        <Link to={`/invoices/${d.id}/edit`} className="btn btn-secondary">
          <Pencil size={15} /> Edit
        </Link>
        <button className="btn btn-primary" onClick={issue} disabled={finalize.isPending}>
          <Send size={15} /> {finalize.isPending ? "Issuing…" : "Issue invoice"}
        </button>
      </>
    );
  } else if (unpaid) {
    actions = (
      <>
        {d.paid === 0 && (
          <button className="btn btn-ghost" onClick={() => setConfirm("void")}>
            <Ban size={15} /> Void
          </button>
        )}
        {print}
        <button className="btn btn-primary" onClick={() => setPaying(true)}>
          <HandCoins size={16} /> Record payment
        </button>
      </>
    );
  } else if (d.status === "paid") {
    actions = print;
  }

  const subtitle =
    d.status === "draft"
      ? "Draft — not on your books yet. Issue it when you're ready to send."
      : d.status === "void"
        ? "This invoice was voided and no longer counts toward your books."
        : d.status === "paid"
          ? "Paid in full. Nice."
          : late > 0
            ? `${plural(late, "day")} past due`
            : late === 0
              ? "Due today"
              : `Due in ${plural(-late, "day")}`;

  return (
    <div className="page">
      <div className="no-print">
        <PageHeader
          back={{ to: "/invoices", label: "Invoices" }}
          title={
            <span className="row" style={{ gap: 12 }}>
              Invoice #{d.number} <StatusPill status={d.status} />
            </span>
          }
          subtitle={
            <span className={d.status === "overdue" ? "neg" : undefined} style={d.status === "overdue" ? { fontWeight: 700 } : undefined}>
              {d.customer_name} · {subtitle}
            </span>
          }
          actions={actions}
        />
      </div>

      <div className="inv-view">
        {/* ---------------------------------------------------- the document */}
        <article className="inv-paper">
          {d.status === "paid" && <div className="p-stamp paid">PAID</div>}
          {d.status === "void" && <div className="p-stamp void">VOID</div>}
          {d.status === "draft" && <div className="p-stamp draft">DRAFT</div>}

          <header className="p-head">
            <div>
              <div className="p-company">{company}</div>
              <div className="p-lines">
                {[prefs["company.address"], prefs["company.email"], prefs["company.phone"]].filter(Boolean).join("\n")}
              </div>
            </div>
            <div>
              <div className="p-title">INVOICE</div>
              <div className="p-number num">#{d.number}</div>
            </div>
          </header>

          <div className="p-meta">
            <div>
              <div className="p-label">Bill to</div>
              <div className="p-billto-name">{c?.name ?? d.customer_name}</div>
              <div className="p-lines" style={{ marginTop: 2 }}>
                {[c?.company && c.company !== c.name ? c.company : null, c?.address, c?.email].filter(Boolean).join("\n")}
              </div>
            </div>
            <dl className="p-dates" style={{ margin: 0 }}>
              <dt>Invoice date</dt>
              <dd>{date(d.issue_date)}</dd>
              <dt>Due date</dt>
              <dd>{date(d.due_date)}</dd>
              <dt>Terms</dt>
              <dd>{termsLabel(daysBetween(d.issue_date, d.due_date))}</dd>
            </dl>
          </div>

          <table>
            <thead>
              <tr>
                <th>Description</th>
                <th className="right" style={{ width: 70 }}>
                  Qty
                </th>
                <th className="right" style={{ width: 110 }}>
                  Rate
                </th>
                <th className="right" style={{ width: 120 }}>
                  Amount
                </th>
              </tr>
            </thead>
            <tbody>
              {d.lines.map((l, i) => (
                <tr key={i}>
                  <td>
                    <div className="p-desc">{l.description || "—"}</div>
                    {l.taxable && d.tax_rate_bp > 0 && <div className="p-taxable">Taxable</div>}
                  </td>
                  <td className="right num">{qty(l.quantity_milli)}</td>
                  <td className="right num">{money(l.unit_price)}</td>
                  <td className="right num" style={{ fontWeight: 700 }}>
                    {money(l.amount)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>

          <div className="p-summary">
            <div className="p-summary-box">
              <div className="p-sum-row">
                <span>Subtotal</span>
                <span className="num">{money(d.subtotal)}</span>
              </div>
              {(d.tax_rate_bp > 0 || d.tax !== 0 || hasTaxable) && (
                <div className="p-sum-row">
                  <span>Tax ({rateLabel(d.tax_rate_bp)})</span>
                  <span className="num">{money(d.tax)}</span>
                </div>
              )}
              <div className="p-sum-row total">
                <span>Total</span>
                <span className="num">{money(d.total)}</span>
              </div>
              {d.paid > 0 && (
                <div className="p-sum-row">
                  <span>Amount paid</span>
                  <span className="num">−{money(d.paid)}</span>
                </div>
              )}
              {d.status !== "void" && (
                <div className={`p-due ${d.status === "paid" ? "settled" : ""}`}>
                  <span className="p-due-label">{d.status === "paid" ? "Paid in full" : "Balance due"}</span>
                  <span className="p-due-amount num">{money(d.status === "draft" ? d.total : d.balance)}</span>
                </div>
              )}
            </div>
          </div>

          {d.memo && <footer className="p-footer">{d.memo}</footer>}
        </article>

        {/* ---------------------------------------------------- sidebar */}
        <aside className="stack no-print">
          {d.status !== "draft" && d.status !== "void" && (
            <Card className="inv-balance-card">
              <div className="stat-label">Balance due</div>
              <div className={`stat-value num ${d.status === "overdue" ? "neg" : d.balance === 0 ? "pos" : ""}`}>{money(d.balance)}</div>
              <div className="stat-foot">
                {d.paid > 0 ? `${money(d.paid)} of ${money(d.total)} paid` : `${money(d.total)} total`}
              </div>
              {d.total > 0 && (
                <div className="progress" style={{ marginTop: 12 }}>
                  <div style={{ width: `${Math.min(100, Math.round((d.paid / d.total) * 100))}%`, background: "var(--pos)" }} />
                </div>
              )}
            </Card>
          )}

          <Card title="Send it">
            <div className="stack-sm">
              {mailto ? (
                <a className="btn btn-secondary btn-block" href={mailto}>
                  <Mail size={15} /> Email to {c?.name}
                </a>
              ) : (
                <div className="subtle small">
                  {contact.isLoading ? "Looking up the customer…" : "Add an email for this customer on the Contacts page to email invoices."}
                </div>
              )}
              <button className="btn btn-ghost btn-block" onClick={copyText}>
                {copied ? <Check size={15} /> : <Copy size={15} />} {copied ? "Copied" : "Copy as text"}
              </button>
              <div className="hint">Tip: use Print / Save PDF to attach a PDF copy.</div>
            </div>
          </Card>

          <Card title="Payments">
            {d.payments.length === 0 ? (
              <div className="subtle small">
                {d.status === "draft" ? "Issue this invoice to start collecting payments." : "No payments recorded yet."}
              </div>
            ) : (
              <div className="stack-sm">
                {d.payments.map((p) => (
                  <div key={p.payment_id} className="row-between">
                    <div>
                      <div className="bold">{date(p.date)}</div>
                      <div className="subtle small">
                        {p.method || "Payment"}
                        {p.reference && ` · ${p.reference}`}
                      </div>
                    </div>
                    <div className="row" style={{ gap: 4 }}>
                      <Money cents={p.amount} className="bold pos" />
                      <VoidPaymentButton paymentId={p.payment_id} amount={p.amount} />
                    </div>
                  </div>
                ))}
                {d.payments.length > 1 && (
                  <div className="row-between" style={{ borderTop: "1px solid var(--line)", paddingTop: 8 }}>
                    <span className="muted small bold">Total paid</span>
                    <Money cents={d.paid} className="bold" />
                  </div>
                )}
              </div>
            )}
            {unpaid && (
              <button className="btn btn-secondary btn-sm" style={{ marginTop: 14 }} onClick={() => setPaying(true)}>
                <HandCoins size={14} /> Record payment
              </button>
            )}
          </Card>
        </aside>
      </div>

      <ReceivePaymentModal open={paying} onClose={() => setPaying(false)} invoiceId={d.id} customerId={d.customer_id} />
      <ConfirmModal
        open={confirm === "delete"}
        onClose={() => setConfirm(null)}
        onConfirm={doDelete}
        title={`Delete draft #${d.number}?`}
        body="This draft was never issued, so deleting it won't change your books. This can't be undone."
        confirmLabel="Delete draft"
        danger
      />
      <ConfirmModal
        open={confirm === "void"}
        onClose={() => setConfirm(null)}
        onConfirm={doVoid}
        title={`Void invoice #${d.number}?`}
        body={`${d.customer_name} will no longer owe ${money(d.balance)}. We'll reverse it in your books on today's date, and keep the invoice for your records.`}
        confirmLabel="Void invoice"
        danger
      />
    </div>
  );
}

function termsLabel(days: number): string {
  if (days <= 0) return "Due on receipt";
  return `Net ${days}`;
}
