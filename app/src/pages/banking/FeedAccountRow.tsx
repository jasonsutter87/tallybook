// One provider account in the link step: where its transactions should go in the books.

import { CreditCard, Landmark } from "lucide-react";
import { SCOPE_LABEL, type useSides } from "../../book";
import { Field } from "../../components/ui";
import { money } from "../../format";
import type { Account, AccountSubtype, FeedAccount, FeedAccountKind, Scope } from "../../types";

export const KIND_LABEL: Record<FeedAccountKind, string> = {
  depository: "Bank account",
  credit: "Credit card",
  loan: "Loan",
  investment: "Investment",
  other: "Account",
};

export interface LinkChoice {
  /** "new" creates a ledger account, "skip" ignores, number links to an existing account. */
  target: "new" | "skip" | number;
  name: string;
  subtype: AccountSubtype;
  scope: Scope;
  start_date: string;
}

export default function FeedAccountRow(props: {
  fa: FeedAccount;
  c: LinkChoice;
  onChange: (patch: Partial<LinkChoice>) => void;
  existing: Account[];
  chosenExisting: Set<number>;
  sides: ReturnType<typeof useSides>;
}) {
  const { fa, c } = props;
  return (
    <div className={`bk-feed-row ${c.target === "skip" ? "skipped" : ""}`}>
      <div className="row-between" style={{ marginBottom: c.target === "skip" ? 0 : 12 }}>
        <div className="row" style={{ gap: 10, minWidth: 0 }}>
          <span className="icon-bubble">{fa.kind === "credit" ? <CreditCard size={16} /> : <Landmark size={16} />}</span>
          <div style={{ minWidth: 0 }}>
            <div className="bold truncate">
              {fa.name} {fa.mask && <span className="subtle">···{fa.mask}</span>}
            </div>
            <div className="subtle small">{KIND_LABEL[fa.kind]}</div>
          </div>
        </div>
        {fa.current_balance != null && <span className="num bold">{money(fa.current_balance)}</span>}
      </div>
      <div className="form-grid">
        <Field label="Link to">
          <select
            className="select"
            value={String(c.target)}
            onChange={(e) => {
              const v = e.target.value;
              props.onChange({ target: v === "new" || v === "skip" ? v : Number(v) });
            }}
          >
            <option value="new">Create a new account</option>
            {props.existing.length > 0 && (
              <optgroup label="Existing accounts">
                {props.existing.map((a) => (
                  <option key={a.id} value={a.id} disabled={props.chosenExisting.has(a.id) && c.target !== a.id}>
                    {a.name}
                  </option>
                ))}
              </optgroup>
            )}
            <option value="skip">Skip — don't import this one</option>
          </select>
        </Field>
        {c.target !== "skip" && (
          <Field label="Start date">
            <input className="input" type="date" value={c.start_date} onChange={(e) => props.onChange({ start_date: e.target.value })} />
          </Field>
        )}
        {c.target === "new" && (
          <Field label="New account name" className={props.sides.both ? undefined : "span-2"}>
            <input className="input" value={c.name} onChange={(e) => props.onChange({ name: e.target.value })} />
          </Field>
        )}
        {c.target === "new" && props.sides.both && (
          <Field label="Whose account is it?">
            <select className="select" value={c.scope} onChange={(e) => props.onChange({ scope: e.target.value as Scope })}>
              {props.sides.sides.map((s) => (
                <option key={s} value={s}>
                  {SCOPE_LABEL[s]}
                </option>
              ))}
            </select>
          </Field>
        )}
      </div>
    </div>
  );
}
