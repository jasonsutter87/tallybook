// Close the books card: the lock date before which nothing can be posted.

import { Lock, LockOpen } from "lucide-react";
import { useEffect, useState } from "react";
import { useAction } from "../../api";
import { ConfirmModal, Field, useToast } from "../../components/ui";
import { date } from "../../format";
import type { ISODate } from "../../types";
import { SettingsCard } from "./shared";

export default function LockCard({ lockDate }: { lockDate: ISODate | null }) {
  const toast = useToast();
  const save = useAction<void, unknown>("settings.set");
  const [value, setValue] = useState<ISODate>(lockDate ?? "");
  const [confirm, setConfirm] = useState<"lock" | "unlock" | null>(null);
  useEffect(() => setValue(lockDate ?? ""), [lockDate]);

  const apply = async (next: ISODate | null) => {
    try {
      await save.mutateAsync({ lock_date: next });
      toast({ text: next ? `Books closed through ${date(next)}` : "Books unlocked" });
    } catch (e) {
      toast.error(e);
    }
  };

  return (
    <SettingsCard
      id="lock"
      icon={<Lock size={18} />}
      title="Close the books"
      desc="Nothing dated on or before this date can be added or changed — use after you file taxes."
    >
      <div className="stack">
        <div className="row">
          {lockDate ? (
            <span className="pill pill-warn">
              <Lock size={12} /> Closed through {date(lockDate)}
            </span>
          ) : (
            <span className="pill pill-pos">
              <LockOpen size={12} /> Books are open — every date can be edited
            </span>
          )}
        </div>
        <div className="row" style={{ alignItems: "flex-end", gap: 10, flexWrap: "wrap" }}>
          <Field label="Lock date">
            <input type="date" className="input" style={{ width: 200 }} value={value} onChange={(e) => setValue(e.target.value)} />
          </Field>
          <button className="btn btn-dark" disabled={!value || value === lockDate || save.isPending} onClick={() => setConfirm("lock")}>
            <Lock size={15} /> {lockDate ? "Move lock date" : "Lock"}
          </button>
          {lockDate && (
            <button className="btn btn-secondary" disabled={save.isPending} onClick={() => setConfirm("unlock")}>
              <LockOpen size={15} /> Unlock
            </button>
          )}
        </div>
        <div className="hint">Tip: lock the last day of a year once your taxes for it are filed, so a stray edit can't change numbers you've already reported.</div>
      </div>

      <ConfirmModal
        open={confirm === "lock"}
        onClose={() => setConfirm(null)}
        title={`Close the books through ${date(value)}?`}
        body="Transactions, invoices, and bills dated on or before this day will be read-only. You can unlock later if you need to fix something."
        confirmLabel="Close the books"
        onConfirm={() => apply(value)}
      />
      <ConfirmModal
        open={confirm === "unlock"}
        onClose={() => setConfirm(null)}
        title="Unlock your books?"
        body="Past transactions will be editable again. Changes to a year you've already filed taxes for may make your books disagree with your return."
        confirmLabel="Unlock"
        danger
        onConfirm={() => apply(null)}
      />
    </SettingsCard>
  );
}
