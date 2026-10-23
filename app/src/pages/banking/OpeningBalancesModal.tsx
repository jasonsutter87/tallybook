// The second half of the link step: offer an opening balance for each brand-new
// ledger account, worked out from the bank's balance.

import { CheckCircle2 } from "lucide-react";
import type { Dispatch, SetStateAction } from "react";
import { ErrorBanner, Field, Modal, MoneyInput, Spinner } from "../../components/ui";
import { date, money } from "../../format";

export interface OpeningDraft {
  account_id: number;
  name: string;
  date: string;
  amount: number | null;
  bank_balance: number;
  include: boolean;
}

export default function OpeningBalancesModal(props: {
  openings: OpeningDraft[];
  setOpenings: Dispatch<SetStateAction<OpeningDraft[] | null>>;
  busy: boolean;
  error: Error | null;
  onSave: () => void;
  onFinish: () => void;
}) {
  return (
    <Modal
      open
      onClose={() => props.onFinish()}
      width={620}
      title="Set starting balances"
      subtitle="So your books match the bank from day one."
      footer={
        <>
          <button className="btn btn-ghost" onClick={() => props.onFinish()} disabled={props.busy}>
            Skip for now
          </button>
          <button className="btn btn-primary" onClick={props.onSave} disabled={props.busy}>
            {props.busy ? <Spinner /> : <CheckCircle2 size={16} />} Save & review transactions
          </button>
        </>
      }
    >
      <div className="stack">
        <ErrorBanner error={props.error} />
        {props.openings.map((o, i) => (
          <div key={o.account_id} className={`bk-feed-row ${o.include ? "" : "skipped"}`}>
            <label className="switch" style={{ marginBottom: 10 }}>
              <input
                type="checkbox"
                checked={o.include}
                onChange={(e) => props.setOpenings((os) => os!.map((x, j) => (j === i ? { ...x, include: e.target.checked } : x)))}
              />
              <span>
                Set opening balance for <b>{o.name}</b>
              </span>
            </label>
            <div className="form-grid">
              <Field label={`Balance as of ${date(o.date)}`} hint={`Worked out from today's bank balance of ${money(o.bank_balance)}.`}>
                <MoneyInput
                  value={o.amount}
                  allowNegative
                  onChange={(v) => props.setOpenings((os) => os!.map((x, j) => (j === i ? { ...x, amount: v } : x)))}
                />
              </Field>
              <Field label="As of">
                <input
                  className="input"
                  type="date"
                  value={o.date}
                  onChange={(e) => props.setOpenings((os) => os!.map((x, j) => (j === i ? { ...x, date: e.target.value } : x)))}
                />
              </Field>
            </div>
          </div>
        ))}
      </div>
    </Modal>
  );
}
