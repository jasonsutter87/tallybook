import { Ban, CheckCircle2, Printer, Wallet } from "lucide-react";
import { useState } from "react";
import { Link, useParams } from "react-router-dom";
import VoidPaymentButton from "../components/VoidPaymentButton";
import { useAction, useApi } from "../api";
import { Avatar, BigMoney, Card, ConfirmModal, EmptyState, ErrorBanner, Loading, Money, PageHeader, StatusPill, useAccountNames, useToast } from "../components/ui";
import PayBillsModal from "../flows/PayBillsModal";
import { date, money } from "../format";
import type { BillDetail, Contact } from "../types";
import { DueHint, useToday } from "./billsShared";
import "./bills.css";

export default function BillView() {
  const { id } = useParams();
  const billId = Number(id);
  const toast = useToast();
  const today = useToday();
  const accountName = useAccountNames();
  const { data, error, isLoading } = useApi<BillDetail>("bills.get", { id: billId }, { enabled: Number.isFinite(billId) });
  const contacts = useApi<Contact[]>("contacts.list");
  const voidBill = useAction<null, { id: number }>("bills.void");
  const [paying, setPaying] = useState(false);
  const [voiding, setVoiding] = useState(false);

  if (isLoading) return <Loading />;
  if (error || !data)
    return (
      <div className="page page-narrow">
        <PageHeader back={{ to: "/bills", label: "Bills" }} title="Bill" />
        <ErrorBanner error={error ?? new Error("This bill couldn't be found.")} />
      </div>
    );

  const b = data.summary;
  const vendor = contacts.data?.find((c) => c.id === b.vendor_id);
  const isVoid = b.status === "void";
  const canPay = !isVoid && b.balance > 0;
  const canVoid = !isVoid && data.payments.length === 0;

  return (
    <div className="page page-narrow">
      <PageHeader
        back={{ to: "/bills", label: "Bills" }}
        title={
          <span className="row" style={{ gap: 12 }}>
            {b.number ? `Bill ${b.number}` : "Bill"} <StatusPill status={b.status} />
          </span>
        }
        subtitle={
          <>
            From <Link to={`/contacts?contact=${b.vendor_id}`}>{b.vendor_name}</Link>
            {canPay && <> · {money(b.balance)} to pay</>}
          </>
        }
        actions={
          <>
            <button className="btn btn-ghost" onClick={() => window.print()}>
              <Printer size={16} /> Print
            </button>
            {canVoid && (
              <button className="btn btn-secondary" onClick={() => setVoiding(true)}>
                <Ban size={16} /> Void
              </button>
            )}
            {canPay && (
              <button className="btn btn-primary" onClick={() => setPaying(true)}>
                <Wallet size={16} /> Pay bill
              </button>
            )}
          </>
        }
      />

      <div className="stack" style={{ gap: 20 }}>
        {isVoid && <div className="banner warn">This bill was voided. It no longer counts toward what you owe.</div>}
        {!isVoid && b.balance === 0 && (
          <div className="banner" style={{ background: "var(--pos-soft)", color: "var(--pos)" }}>
            <CheckCircle2 size={18} /> Paid in full. Nice work.
          </div>
        )}

        <section className="card bk-doc" style={isVoid ? { opacity: 0.7 } : undefined}>
          <div className="row-between" style={{ alignItems: "flex-start" }}>
            <div className="row" style={{ gap: 14, alignItems: "flex-start" }}>
              <Avatar name={b.vendor_name} />
              <div>
                <div className="label">Bill from</div>
                <div className="bold" style={{ fontSize: 18, letterSpacing: "-.02em" }}>
                  {b.vendor_name}
                </div>
                {vendor?.email && <div className="small muted">{vendor.email}</div>}
                {vendor?.address && (
                  <div className="small muted" style={{ whiteSpace: "pre-line" }}>
                    {vendor.address}
                  </div>
                )}
              </div>
            </div>
            <div style={{ textAlign: "right" }}>
              <div className="label">{isVoid ? "Voided" : "Balance due"}</div>
              <div style={{ fontSize: 34, color: b.balance > 0 && b.status === "overdue" ? "var(--neg)" : undefined }}>
                <BigMoney cents={isVoid ? 0 : b.balance} className="bk-big" />
              </div>
            </div>
          </div>

          <div className="bk-meta">
            <div>
              <span className="label">Bill #</span>
              <span className="bold">{b.number || "—"}</span>
            </div>
            <div>
              <span className="label">Bill date</span>
              <span className="bold">{date(b.issue_date)}</span>
            </div>
            <div>
              <span className="label">Due date</span>
              <span className="bold">{date(b.due_date)}</span>
              <DueHint bill={b} today={today} />
            </div>
            <div>
              <span className="label">Total</span>
              <span className="bold num">{money(b.total)}</span>
            </div>
          </div>

          <table className="table" style={{ marginTop: 6 }}>
            <thead>
              <tr>
                <th style={{ paddingLeft: 0 }}>Description</th>
                <th>Category</th>
                <th className="right" style={{ paddingRight: 0 }}>
                  Amount
                </th>
              </tr>
            </thead>
            <tbody>
              {data.lines.map((l, i) => (
                <tr key={i}>
                  <td style={{ paddingLeft: 0 }}>{l.description || <span className="subtle">No description</span>}</td>
                  <td>
                    <Link to={`/accounts/${l.account_id}`} className="pill">
                      {accountName(l.account_id)}
                    </Link>
                  </td>
                  <td className="right" style={{ paddingRight: 0 }}>
                    <Money cents={l.amount} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>

          <div className="bk-totals">
            <div className="row-between">
              <span className="muted">Total</span>
              <Money cents={b.total} className="bold" />
            </div>
            <div className="row-between">
              <span className="muted">Paid</span>
              <span className="num bold" style={{ color: b.paid ? "var(--pos)" : undefined }}>
                {b.paid ? `−${money(b.paid)}` : money(0)}
              </span>
            </div>
            <div className="row-between due">
              <span>Balance due</span>
              <span className="num">{money(isVoid ? 0 : b.balance)}</span>
            </div>
          </div>

          {b.memo && (
            <div style={{ marginTop: 22 }}>
              <div className="label">Memo</div>
              <div className="muted" style={{ whiteSpace: "pre-line" }}>
                {b.memo}
              </div>
            </div>
          )}
        </section>

        <Card title="Payments" pad={false}>
          {data.payments.length === 0 ? (
            <EmptyState
              icon={<Wallet size={22} />}
              title="No payments yet"
              body={canPay ? "When you pay this bill, record it here so your books know it's settled." : undefined}
              action={
                canPay ? (
                  <button className="btn btn-primary" onClick={() => setPaying(true)}>
                    Pay {money(b.balance)}
                  </button>
                ) : undefined
              }
            />
          ) : (
            <div style={{ paddingTop: 8 }}>
              {data.payments.map((p) => (
                <div key={p.payment_id} className="list-row">
                  <span className="icon-bubble pos">
                    <CheckCircle2 size={18} />
                  </span>
                  <div className="grow">
                    <div className="bold">Paid by {p.method || "payment"}</div>
                    <div className="small muted">
                      {date(p.date)}
                      {p.reference && ` · ${p.reference}`}
                    </div>
                  </div>
                  <Money cents={p.amount} className="bold" />
                  <VoidPaymentButton paymentId={p.payment_id} amount={p.amount} />
                </div>
              ))}
            </div>
          )}
        </Card>
      </div>

      <PayBillsModal open={paying} onClose={() => setPaying(false)} billId={b.id} />
      <ConfirmModal
        open={voiding}
        onClose={() => setVoiding(false)}
        danger
        title="Void this bill?"
        confirmLabel="Void bill"
        body={
          <>
            This removes the {money(b.total)} bill from {b.vendor_name} from what you owe. It stays in your records, marked void. To fix a mistake,
            void it and enter the bill again.
          </>
        }
        onConfirm={async () => {
          try {
            await voidBill.mutateAsync({ id: b.id });
            toast({ text: "Bill voided" });
          } catch (e) {
            toast.error(e);
          }
        }}
      />
    </div>
  );
}
