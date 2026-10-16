import { Info, Plus, Trash2, UserPlus } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { useAction, useApi } from "../api";
import { AccountSelect, BigMoney, ContactSelect, ErrorBanner, Field, MoneyInput, PageHeader, useToast } from "../components/ui";
import ContactModal from "../flows/ContactModal";
import PayBillsModal from "../flows/PayBillsModal";
import { addDays, daysBetween, money } from "../format";
import type { Account, AccountKind, AccountSubtype, Cents, NewBill } from "../types";
import { TERMS, useToday } from "./billsShared";
import "./bills.css";

type Line = { key: number; description: string; amount: Cents | null; account_id: number | null };

let nextKey = 1;
const newLine = (account_id: number | null): Line => ({ key: nextKey++, description: "", amount: null, account_id });

export default function BillEditor() {
  const navigate = useNavigate();
  const toast = useToast();
  const [params] = useSearchParams();
  const today = useToday();
  const accounts = useApi<Account[]>("accounts.list");
  const create = useAction<number, NewBill>("bills.create");

  const preVendor = Number(params.get("vendor"));
  const [vendorId, setVendorId] = useState<number | null>(Number.isFinite(preVendor) && preVendor > 0 ? preVendor : null);
  const [number, setNumber] = useState("");
  const [issueDate, setIssueDate] = useState(today);
  const [terms, setTerms] = useState<number | "custom">(30);
  const [dueDate, setDueDate] = useState(addDays(today, 30));
  const [memo, setMemo] = useState("");
  const [lines, setLines] = useState<Line[]>(() => [newLine(null)]);
  const [error, setError] = useState<Error | null>(null);
  const [addingVendor, setAddingVendor] = useState(false);
  const [payBill, setPayBill] = useState<number | null>(null);

  // Default category: "Uncategorized Expense" (6999) when the chart has it.
  const defaultAccount = useMemo(() => {
    const list = (accounts.data ?? []).filter((a) => !a.archived);
    return list.find((a) => a.code === "6999")?.id ?? null;
  }, [accounts.data]);
  const seeded = useRef(false);
  useEffect(() => {
    if (seeded.current || defaultAccount == null) return;
    seeded.current = true;
    setLines((ls) => ls.map((l) => (l.account_id == null ? { ...l, account_id: defaultAccount } : l)));
  }, [defaultAccount]);

  // The book's "today" may arrive after first render.
  const touchedDate = useRef(false);
  useEffect(() => {
    if (touchedDate.current) return;
    setIssueDate(today);
    if (terms !== "custom") setDueDate(addDays(today, terms));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [today]);

  const total = lines.reduce((s, l) => s + (l.amount ?? 0), 0);

  const changeIssue = (d: string) => {
    touchedDate.current = true;
    setIssueDate(d);
    if (terms !== "custom" && d) setDueDate(addDays(d, terms));
  };
  const changeTerms = (v: string) => {
    if (v === "custom") return setTerms("custom");
    const n = Number(v);
    setTerms(n);
    if (issueDate) setDueDate(addDays(issueDate, n));
  };
  const changeDue = (d: string) => {
    touchedDate.current = true;
    setDueDate(d);
    const match = TERMS.find((t) => issueDate && d && daysBetween(issueDate, d) === t.value);
    setTerms(match ? match.value : "custom");
  };

  const setLine = (key: number, patch: Partial<Line>) => setLines((ls) => ls.map((l) => (l.key === key ? { ...l, ...patch } : l)));

  const validate = (): NewBill | null => {
    if (!vendorId) return fail("Choose the vendor who sent this bill.");
    if (!issueDate || !dueDate) return fail("Enter the bill date and due date.");
    if (dueDate < issueDate) return fail("The due date can't be before the bill date.");
    const used = lines.filter((l) => l.description.trim() || l.amount);
    if (!used.length) return fail("Add at least one line with an amount.");
    for (const [i, l] of used.entries()) {
      if (!l.amount || l.amount <= 0) return fail(`Line ${i + 1} needs an amount.`);
      if (!l.account_id) return fail(`Line ${i + 1} needs a category.`);
    }
    return {
      vendor_id: vendorId,
      number: number.trim(),
      issue_date: issueDate,
      due_date: dueDate,
      memo: memo.trim(),
      lines: used.map((l) => ({ description: l.description.trim(), amount: l.amount!, account_id: l.account_id! })),
    };
  };
  function fail(msg: string): null {
    setError(new Error(msg));
    return null;
  }

  const save = async (andPay: boolean) => {
    setError(null);
    const bill = validate();
    if (!bill) return;
    try {
      const id = await create.mutateAsync(bill);
      toast({ text: `Bill for ${money(total)} saved` });
      if (andPay) setPayBill(id);
      else navigate(`/bills/${id}`);
    } catch (e) {
      setError(e as Error);
    }
  };

  return (
    <div className="page page-narrow">
      <PageHeader back={{ to: "/bills", label: "Bills" }} title="Enter a bill" subtitle="Record a bill a vendor sent you." />

      <div className="stack" style={{ gap: 20 }}>
        <ErrorBanner error={error} />

        <div className="card card-pad stack" style={{ gap: 18 }}>
          <div className="form-grid">
            <Field label="Vendor" className="span-2">
              <div className="row" style={{ gap: 8 }}>
                <div className="grow">
                  <ContactSelect kind="vendor" value={vendorId} onChange={setVendorId} />
                </div>
                <button className="btn btn-secondary" type="button" onClick={() => setAddingVendor(true)}>
                  <UserPlus size={15} /> New vendor
                </button>
              </div>
            </Field>
            <Field label="Bill number" hint="The vendor's invoice or reference number">
              <input className="input" value={number} onChange={(e) => setNumber(e.target.value)} placeholder="Optional" />
            </Field>
            <Field label="Bill date">
              <input className="input" type="date" value={issueDate} onChange={(e) => changeIssue(e.target.value)} />
            </Field>
            <Field label="Terms">
              <select className="select" value={String(terms)} onChange={(e) => changeTerms(e.target.value)}>
                {TERMS.map((t) => (
                  <option key={t.value} value={t.value}>
                    {t.label}
                  </option>
                ))}
                <option value="custom">Custom</option>
              </select>
            </Field>
            <Field label="Due date">
              <input className="input" type="date" value={dueDate} min={issueDate} onChange={(e) => changeDue(e.target.value)} />
            </Field>
          </div>
        </div>

        <div className="card card-pad stack" style={{ gap: 12 }}>
          <div className="row-between">
            <h3 className="card-title">What's it for?</h3>
            <span className="subtle small">Pick a category for each line — use an asset account for equipment.</span>
          </div>
          <div className="bk-lines bk-line-head">
            <span>Description</span>
            <span className="bk-line-account">Category</span>
            <span className="right">Amount</span>
            <span />
          </div>
          {lines.map((l) => (
            <div key={l.key} className="bk-lines">
              <input className="input" placeholder="e.g. October design work" value={l.description} onChange={(e) => setLine(l.key, { description: e.target.value })} />
              <div className="bk-line-account">
                <AccountSelect value={l.account_id} onChange={(id) => setLine(l.key, { account_id: id })} kinds={EXPENSE_KINDS} subtypes={EXPENSE_SUBTYPES} />
              </div>
              <MoneyInput value={l.amount} onChange={(c) => setLine(l.key, { amount: c })} />
              <button
                className="btn btn-ghost btn-icon"
                type="button"
                aria-label="Remove line"
                disabled={lines.length === 1}
                onClick={() => setLines((ls) => ls.filter((x) => x.key !== l.key))}
              >
                <Trash2 size={16} />
              </button>
            </div>
          ))}
          <div className="row-between" style={{ marginTop: 4, alignItems: "flex-end" }}>
            <button className="btn btn-ghost btn-sm" type="button" onClick={() => setLines((ls) => [...ls, newLine(defaultAccount)])}>
              <Plus size={15} /> Add a line
            </button>
            <div style={{ textAlign: "right" }}>
              <div className="label">Bill total</div>
              <div style={{ fontSize: 32 }}>
                <BigMoney cents={total} className="bk-big" />
              </div>
            </div>
          </div>
        </div>

        <div className="card card-pad">
          <Field label="Memo" hint="Optional — a note for yourself">
            <textarea className="textarea" rows={2} value={memo} onChange={(e) => setMemo(e.target.value)} />
          </Field>
        </div>

        <div className="row-between" style={{ flexWrap: "wrap", gap: 14 }}>
          <div className="row small muted" style={{ gap: 8, maxWidth: 420 }}>
            <Info size={15} style={{ flexShrink: 0 }} />
            Bills are recorded as money you owe until you pay them.
          </div>
          <div className="row">
            <button className="btn btn-ghost" type="button" onClick={() => navigate("/bills")}>
              Cancel
            </button>
            <button className="btn btn-secondary" type="button" disabled={create.isPending} onClick={() => save(true)}>
              Save &amp; pay now
            </button>
            <button className="btn btn-primary" type="button" disabled={create.isPending} onClick={() => save(false)}>
              {create.isPending ? "Saving…" : "Save bill"}
            </button>
          </div>
        </div>
      </div>

      <ContactModal open={addingVendor} onClose={() => setAddingVendor(false)} defaultKind="vendor" onSaved={(id) => setVendorId(id)} />
      <PayBillsModal
        open={payBill != null}
        billId={payBill ?? undefined}
        onClose={() => {
          const id = payBill;
          setPayBill(null);
          if (id != null) navigate(`/bills/${id}`);
        }}
      />
    </div>
  );
}

// Expense accounts of any flavor, plus asset accounts a bill can sensibly buy
// (equipment, prepaid items). Excludes bank/AR so lines can't hit them by mistake.
const EXPENSE_SUBTYPES: AccountSubtype[] = ["expense", "other_expense", "cost_of_goods_sold", "fixed_asset", "other_current_asset"];
const EXPENSE_KINDS: AccountKind[] = ["expense", "asset"];
