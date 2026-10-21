// Log money in, out, or between accounts by hand: a cash till, a check,
// anything the bank feed won't see. A few plain questions, and the backend
// writes a proper balanced entry. Works on either side of the book.

import { useEffect, useState } from "react";
import { useAction, useApi } from "../api";
import { useSides } from "../book";
import { PersonSelect } from "../components/household";
import { AccountSelect, ContactSelect, ErrorBanner, Field, Modal, MoneyInput, Tabs, useToast } from "../components/ui";
import { money, todayISO } from "../format";
import type { Account, AccountSubtype, Cents, Scope } from "../types";

type Way = "out" | "in" | "move";

/** Accounts money physically sits in or is charged to. */
const MONEY: AccountSubtype[] = ["bank", "credit_card", "other_current_asset"];

/** A till or cash box first, then any bank account, on the given side. */
function defaultAccount(accounts: Account[], scope: Scope): number | null {
  const live = accounts.filter((a) => a.scope === scope && !a.archived && MONEY.includes(a.subtype));
  const cash = live.find((a) => /cash|till|drawer|petty/i.test(a.name));
  return (cash ?? live.find((a) => a.subtype === "bank") ?? live[0])?.id ?? null;
}

export default function LogMoneyModal(props: {
  open: boolean;
  onClose: () => void;
  /** Which side of the book; the book's main side when not given. */
  scope?: Scope;
  /** Start with this account as where the money is. */
  accountId?: number | null;
  personId?: number | null;
  date?: string;
}) {
  const toast = useToast();
  const sides = useSides();
  const post = useAction<number>("entries.post");
  const accounts = useApi<Account[]>("accounts.list");
  const scope = props.scope ?? sides.main;
  const household = scope === "household";
  const [way, setWay] = useState<Way>("out");
  const [amount, setAmount] = useState<Cents | null>(null);
  const [category, setCategory] = useState<number | null>(null);
  const [from, setFrom] = useState<number | null>(null);
  const [to, setTo] = useState<number | null>(null);
  const [person, setPerson] = useState<number | null>(null);
  const [payee, setPayee] = useState<number | null>(null);
  const [date, setDate] = useState(todayISO());
  const [memo, setMemo] = useState("");

  useEffect(() => {
    if (!props.open) return;
    setWay("out");
    setAmount(null);
    setCategory(null);
    setTo(null);
    setPerson(props.personId ?? null);
    setPayee(null);
    setDate(props.date ?? todayISO());
    setMemo("");
    post.reset();
    setFrom(props.accountId ?? defaultAccount(accounts.data ?? [], scope));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.open]);

  const moving = way === "move";
  const ready = !!amount && amount > 0 && from != null && !!date && (moving ? to != null && to !== from : category != null);

  const submit = async () => {
    if (!ready) return;
    // Raw signs: positive is a debit. Money arriving debits the account it lands in.
    const [debit, credit] = way === "out" ? [category!, from!] : way === "in" ? [from!, category!] : [to!, from!];
    const fallback = { out: household ? "Spending" : "Cash expense", in: household ? "Money in" : "Cash sale", move: "Moved money" }[way];
    try {
      await post.mutateAsync({
        date,
        memo: memo.trim() || fallback,
        lines: [
          { account_id: debit, amount },
          { account_id: credit, amount: -amount },
        ],
        person_id: household && !moving ? person : null,
        payee_id: !household && !moving ? payee : null,
      });
      toast({ text: `${money(amount)} ${{ out: "out", in: "in", move: "moved" }[way]} · logged` });
      props.onClose();
    } catch {
      /* shown inline */
    }
  };

  const tabs: { value: Way; label: string }[] = household
    ? [
        { value: "out", label: "Money out" },
        { value: "in", label: "Money in" },
        { value: "move", label: "Move money" },
      ]
    : [
        { value: "in", label: "Sale or money in" },
        { value: "out", label: "Expense" },
        { value: "move", label: "Deposit or move" },
      ];

  return (
    <Modal
      open={props.open}
      onClose={props.onClose}
      title={household ? "Log money" : "Log cash"}
      subtitle={household ? "For cash and anything your bank won't send over." : "For the till, cash on hand, checks, and anything the bank feed won't see."}
      footer={
        <>
          <button className="btn btn-secondary" onClick={props.onClose}>
            Cancel
          </button>
          <button className="btn btn-primary" disabled={!ready || post.isPending} onClick={submit}>
            {post.isPending ? "Saving…" : "Log it"}
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
        <ErrorBanner error={post.error} />
        <Tabs<Way>
          value={way}
          onChange={(w) => {
            setWay(w);
            setCategory(null);
            setPayee(null);
          }}
          tabs={tabs}
        />
        <Field label="How much?">
          <MoneyInput value={amount} onChange={setAmount} autoFocus />
        </Field>
        {!moving && (
          <Field label={way === "out" ? "What was it?" : household ? "What kind of money?" : "What was it for?"}>
            <AccountSelect
              scope={scope}
              value={category}
              onChange={setCategory}
              kinds={way === "out" ? ["expense"] : ["income"]}
              placeholder={way === "out" ? (household ? "e.g. Groceries" : "e.g. Office Supplies") : household ? "e.g. Paychecks" : "e.g. Sales"}
            />
          </Field>
        )}
        <div className="form-grid">
          <Field label={moving ? "From" : way === "out" ? "Paid from" : "Went into"}>
            <AccountSelect scope={scope} value={from} onChange={setFrom} subtypes={MONEY} placeholder="e.g. Cash till" />
          </Field>
          {moving ? (
            <Field label="To">
              <AccountSelect scope={scope} value={to} onChange={setTo} subtypes={MONEY} exclude={from != null ? [from] : []} placeholder="e.g. Checking" />
            </Field>
          ) : (
            <Field label="When">
              <input className="input" type="date" value={date} onChange={(e) => setDate(e.target.value)} />
            </Field>
          )}
        </div>
        {moving && (
          <Field label="When">
            <input className="input" type="date" value={date} onChange={(e) => setDate(e.target.value)} />
          </Field>
        )}
        {household && !moving && (
          <Field label="Who was it for?" hint="Leave as shared for the whole household.">
            <PersonSelect value={person} onChange={setPerson} />
          </Field>
        )}
        {!household && !moving && (
          <Field
            label={way === "in" ? "Customer (optional)" : "Paid to (optional)"}
            hint={way === "out" ? "Cash paid to a contractor still counts toward their 1099." : undefined}
          >
            <ContactSelect value={payee} onChange={setPayee} kind={way === "in" ? "customer" : "vendor"} placeholder={way === "in" ? "Walk-in" : "Choose a vendor or contractor"} />
          </Field>
        )}
        <Field label="Note (optional)">
          <input
            className="input"
            value={memo}
            onChange={(e) => setMemo(e.target.value)}
            placeholder={moving ? "e.g. Bank deposit" : household ? "e.g. Farmers market" : "e.g. Saturday market sales"}
          />
        </Field>
        <button type="submit" hidden />
      </form>
    </Modal>
  );
}
