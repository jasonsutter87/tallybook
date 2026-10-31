// About your data: where the book lives and what leaves the machine.

import { Check, ShieldCheck } from "lucide-react";
import { useApi } from "../../api";
import type { BookStatus } from "../../types";
import { SettingsCard } from "./shared";

export default function DataCard() {
  const status = useApi<BookStatus>("book.status");
  return (
    <SettingsCard id="data" icon={<ShieldCheck size={18} />} title="About your data" desc="Your books live in a single file on your computer. You own it.">
      <div className="stack">
        <div>
          <div className="label" style={{ marginBottom: 6 }}>
            Book file
          </div>
          <div className="path-box">{status.data?.path ?? "—"}</div>
        </div>
        <ul className="privacy-list">
          <li>
            <Check size={16} />
            <span>
              <b>Stored on your computer.</b> <span className="muted">There's no Tallybook cloud and no account to sign up for.</span>
            </span>
          </li>
          <li>
            <Check size={16} />
            <span>
              <b>No tracking.</b> <span className="muted">Tallybook doesn't send analytics or usage data anywhere.</span>
            </span>
          </li>
          <li>
            <Check size={16} />
            <span>
              <b>Bank connections are opt-in.</b> <span className="muted">If you link a bank, transactions come straight from your provider to this computer.</span>
            </span>
          </li>
          <li>
            <Check size={16} />
            <span>
              <b>Calendar reminders are opt-in.</b>{" "}
              <span className="muted">If you connect a calendar, the names and due dates of upcoming bills (and amounts, if you turn them on) go to Google or Microsoft. Nothing else in the book does.</span>
            </span>
          </li>
          <li>
            <Check size={16} />
            <span>
              <b>Open format.</b> <span className="muted">The book is a standard SQLite database — readable by many tools, today and decades from now.</span>
            </span>
          </li>
        </ul>
      </div>
    </SettingsCard>
  );
}
