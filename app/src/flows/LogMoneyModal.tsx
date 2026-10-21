// Log money in or out by hand: cash, a check, anything the bank feed won't
// see. Four questions (how much, on what, from where, for whom) and the
// backend writes a proper balanced entry.

import { useEffect, useState } from "react";
import { useAction, useApi } from "../api";
import { PersonSelect } from "../components/household";
import { AccountSelect, ErrorBanner, Field, Modal, MoneyInput, Tabs, useToast } from "../components/ui";
import { money, todayISO } from "../format";
import type { Account, Cents } from "../types";

type Way = "out" | "in";

export default function LogMoneyModal(props: { open: boolean; onClose: () => void; personId?: number | null; date?: string }) {
  const toast = useToast();
  const post = useAction<number>("entries.post");
  const accounts = useApi<Account[]>("accounts.list");
  const [way, setWay] = useState<Way>("out");
  const [amount, setAmount] = useState<Cents | null>(null);
  const [category, setCategory] = useState<number | null>(null);
  const [from, setFrom] = useState<number | null>(null);
  const [person, setPerson] = useState<number | null>(null);
  const [date, setDate] = useState(todayISO());
  const [memo, setMemo] = useState("");

  useEffect(() => {
    if (!props.open) return;
    setWay("out");
    setAmount(null);
    setCategory(null);
    setPerson(props.personId ?? null);
    setDate(props.date ?? todayISO());
    setMemo("");
    post.reset();
    // Default to the household's first bank account.
    const bank = (accounts.data ?? []).find((a) => a.scope === "household" && a.subtype === "bank" && !a.archived);
    setFrom(bank?.id ?? null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.open]);

  const ready = !!amount && amount > 0 && category != null && from != null && !!date;

  const submit = async () => {
    if (!ready) return;
    const lines =
      way === "out"
        ? [
            { account_id: category, amount },
            { account_id: from, amount: -amount },
          ]
        : [
            { account_id: from, amount },
            { account_id: category, amount: -amount },
          ];
    try {
      await post.mutateAsync({ date, memo: memo.trim() || (way === "out" ? "Spending" : "Money in"), lines, person_id: person });
      toast({ text: `${money(amount)} ${way === "out" ? "spent" : "in"} logged` });
      props.onClose();
    } catch {
      /* shown inline */
    }
  };

  return (
    <Modal
      open={props.open}
      onClose={props.onClose}
      title="Log money"
      subtitle="For cash and anything your bank won't send over."
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
          }}
          tabs={[
            { value: "out", label: "Money out" },
            { value: "in", label: "Money in" },
          ]}
        />
        <Field label="How much?">
          <MoneyInput value={amount} onChange={setAmount} autoFocus />
        </Field>
        <Field label={way === "out" ? "What was it?" : "What kind of money?"}>
          <AccountSelect
            scope="household"
            value={category}
            onChange={setCategory}
            kinds={way === "out" ? ["expense"] : ["income"]}
            placeholder={way === "out" ? "e.g. Groceries" : "e.g. Paychecks"}
          />
        </Field>
        <div className="form-grid">
          <Field label={way === "out" ? "Paid from" : "Went into"}>
            <AccountSelect scope="household" value={from} onChange={setFrom} subtypes={["bank", "credit_card", "other_current_asset"]} placeholder="Account" />
          </Field>
          <Field label="When">
            <input className="input" type="date" value={date} onChange={(e) => setDate(e.target.value)} />
          </Field>
        </div>
        <Field label="Who was it for?" hint="Leave as shared for the whole household.">
          <PersonSelect value={person} onChange={setPerson} />
        </Field>
        <Field label="Note (optional)">
          <input className="input" value={memo} onChange={(e) => setMemo(e.target.value)} placeholder="e.g. Farmers market" />
        </Field>
        <button type="submit" hidden />
      </form>
    </Modal>
  );
}
