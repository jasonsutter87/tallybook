// Adds an account to the chart. Balance-sheet accounts can start with an
// opening balance, which the backend posts against Opening Balance Equity.

import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useAction, useApi } from "../api";
import { SCOPE_LABEL, useSides } from "../book";
import { ErrorBanner, Field, Modal, MoneyInput, Tabs, useToast } from "../components/ui";
import { FLEX_LABEL, SUBTYPE_LABEL, todayISO } from "../format";
import type { AccountKind, AccountSubtype, Cents, Flex, Scope, SubtypeInfo } from "../types";
import "../pages/accounts.css";

export const SUBTYPE_HELP: Record<AccountSubtype, string> = {
  bank: "Checking, savings, PayPal, cash box",
  accounts_receivable: "Money customers owe you (tracked by invoices)",
  other_current_asset: "Deposits, prepaid expenses, undeposited funds",
  fixed_asset: "Computers, furniture, vehicles, equipment",
  credit_card: "Business credit and charge cards",
  accounts_payable: "Money you owe vendors (tracked by bills)",
  other_current_liability: "Sales tax, payroll owed, short-term debts",
  long_term_liability: "Loans, lines of credit, financing",
  equity: "Owner investments and draws",
  retained_earnings: "Profit kept in the business from past years",
  income: "Sales, services, fees you charge",
  other_income: "Interest, refunds, one-off gains",
  cost_of_goods_sold: "Materials and costs of what you sell",
  expense: "Rent, software, meals, travel",
  other_expense: "Interest paid, one-off losses",
};

const GROUP_LABEL: Record<AccountKind, string> = {
  asset: "Money & things you own",
  liability: "Cards, loans & what you owe",
  equity: "Owner's equity",
  income: "Income",
  expense: "Expenses",
};

/**
 * A household has no customers, vendors on account, or inventory. Retained
 * earnings is never picked by hand on either side; the books keep it.
 */
const BUSINESS_ONLY: AccountSubtype[] = ["accounts_receivable", "accounts_payable", "cost_of_goods_sold", "retained_earnings"];

const FLEX_HELP: Record<Flex, string> = {
  fixed: "Stays the same each month, and is hard to change: rent, a car payment.",
  usage: "Comes every month but moves with how much you use: power, gas, groceries.",
  lifestyle: "Comes down to choices: eating out, shopping, fun.",
};

/** Subtypes where an opening balance makes sense. AR/AP come from invoices/bills. */
const OPENING_OK: AccountSubtype[] = [
  "bank",
  "other_current_asset",
  "fixed_asset",
  "credit_card",
  "other_current_liability",
  "long_term_liability",
];

export default function AddAccountModal(props: {
  open: boolean;
  onClose: () => void;
  defaultSubtype?: AccountSubtype;
  onCreated?: (id: number) => void;
  /** Which side of the book; the book's main side when not given. */
  scope?: Scope;
}) {
  const sides = useSides();
  const toast = useToast();
  const navigate = useNavigate();
  const subtypes = useApi<SubtypeInfo[]>("accounts.subtypes");
  const create = useAction<number>("accounts.create");

  const [name, setName] = useState("");
  const [subtype, setSubtype] = useState<AccountSubtype>(props.defaultSubtype ?? "bank");
  const [code, setCode] = useState("");
  const [opening, setOpening] = useState<Cents | null>(null);
  const [openingDate, setOpeningDate] = useState(todayISO());
  const [scope, setScope] = useState<Scope>(props.scope ?? sides.main);
  const [flex, setFlex] = useState<Flex | "">("");

  useEffect(() => {
    if (props.open) {
      setScope(props.scope ?? sides.main);
      setFlex("");
      setName("");
      setSubtype(props.defaultSubtype ?? "bank");
      setCode("");
      setOpening(null);
      setOpeningDate(todayISO());
      create.reset();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.open, props.defaultSubtype, props.scope]);

  const groups = useMemo(() => {
    const list = subtypes.data ?? [];
    const kinds: AccountKind[] = ["asset", "liability", "equity", "income", "expense"];
    const hidden: AccountSubtype[] = scope === "household" ? BUSINESS_ONLY : ["retained_earnings"];
    return kinds.map((k) => ({
      kind: k,
      label: scope === "household" && k === "expense" ? "Spending" : GROUP_LABEL[k],
      items: list.filter((s) => s.kind === k && !hidden.includes(s.value)),
    }));
  }, [subtypes.data, scope]);
  // Only household spending categories say how far they can bend (flex).
  const spending = scope === "household" && (subtype === "expense" || subtype === "other_expense");

  // Switching to the household side drops a business-only choice.
  useEffect(() => {
    if (scope === "household" && BUSINESS_ONLY.includes(subtype)) setSubtype("bank");
  }, [scope, subtype]);

  const showOpening = OPENING_OK.includes(subtype);
  const owed = subtype === "credit_card" || subtype === "other_current_liability" || subtype === "long_term_liability";

  const submit = async () => {
    if (!name.trim()) return;
    try {
      const id = await create.mutateAsync({
        code: code.trim() || null,
        name: name.trim(),
        subtype,
        scope,
        flex: spending && flex ? flex : null,
        opening_balance: showOpening && opening ? opening : null,
        opening_date: showOpening && opening ? openingDate : null,
      });
      toast({ text: `${name.trim()} added` });
      props.onClose();
      props.onCreated?.(id);
      if (subtype === "bank" || subtype === "credit_card") navigate(`/accounts/${id}`);
    } catch {
      /* shown inline */
    }
  };

  return (
    <Modal
      open={props.open}
      onClose={props.onClose}
      title="Add an account"
      subtitle="Accounts are the buckets your money moves between."
      footer={
        <>
          <button className="btn btn-secondary" onClick={props.onClose}>
            Cancel
          </button>
          <button className="btn btn-primary" disabled={!name.trim() || create.isPending} onClick={submit}>
            {create.isPending ? "Adding…" : "Add account"}
          </button>
        </>
      }
    >
      <form
        className="stack"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <ErrorBanner error={create.error} />
        {sides.both && (
          <Field label="Which side of the book?">
            <Tabs<Scope>
              value={scope}
              onChange={setScope}
              tabs={sides.sides.map((s) => ({ value: s, label: SCOPE_LABEL[s] }))}
            />
          </Field>
        )}
        <Field label="Name">
          <input
            className="input"
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder={scope === "household" ? "e.g. Golden 1 Checking" : "e.g. Chase Business Checking"}
          />
        </Field>
        <Field label="What kind of account is it?">
          <select className="select" value={subtype} onChange={(e) => setSubtype(e.target.value as AccountSubtype)}>
            {groups.map(
              (g) =>
                g.items.length > 0 && (
                  <optgroup key={g.kind} label={g.label}>
                    {g.items.map((s) => (
                      <option key={s.value} value={s.value}>
                        {SUBTYPE_LABEL[s.value]}
                      </option>
                    ))}
                  </optgroup>
                ),
            )}
          </select>
          <div className="type-desc">
            <span className="bold" style={{ color: "var(--ink)" }}>
              {SUBTYPE_LABEL[subtype]}
            </span>{" "}
            — {SUBTYPE_HELP[subtype]}
          </div>
        </Field>
        {spending && (
          <Field label="How much can it bend?" hint={flex ? FLEX_HELP[flex] : "Used to point out where there's room to save."}>
            <select className="select" value={flex} onChange={(e) => setFlex(e.target.value as Flex | "")}>
              <option value="">Not sure</option>
              {(Object.keys(FLEX_LABEL) as Flex[]).map((f) => (
                <option key={f} value={f}>
                  {FLEX_LABEL[f]}
                </option>
              ))}
            </select>
          </Field>
        )}
        <Field label="Account number (optional)" hint="A short code to sort your chart, like 1020. Leave blank if you don't use codes.">
          <input className="input" value={code} onChange={(e) => setCode(e.target.value)} placeholder="e.g. 1020" style={{ maxWidth: 160 }} />
        </Field>
        {showOpening && (
          <div className="form-grid">
            <Field label={owed ? "Amount owed today (optional)" : "Opening balance (optional)"} hint={owed ? "What you owe as of the date" : "What's in it as of the date"}>
              <MoneyInput value={opening} onChange={setOpening} allowNegative={subtype === "bank"} />
            </Field>
            <Field label="As of">
              <input className="input" type="date" value={openingDate} onChange={(e) => setOpeningDate(e.target.value)} />
            </Field>
          </div>
        )}
        <button type="submit" hidden />
      </form>
    </Modal>
  );
}
