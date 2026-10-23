// The book's sides: turn the business or household side on or off.

import { Home, Plus, Check } from "lucide-react";
import { useState } from "react";
import { useAction } from "../../api";
import { SCOPE_LABEL, useSides } from "../../book";
import { ConfirmModal, useToast } from "../../components/ui";
import type { Scope } from "../../types";
import { UpgradeOffer, useLicense } from "../../flows/License";
import { SettingsCard } from "./shared";

/** Why someone would add the side their book doesn't keep yet. */
const SIDE_PITCH: Record<Scope, string> = {
  business: "Invoices, bills, business reports and tax prep, kept apart from the household's numbers.",
  household: "A monthly budget, bills coming up, and goals, kept apart from the business's numbers.",
};

/** Which sides this book keeps, and adding the other one. */
export default function SidesCard() {
  const toast = useToast();
  const sides = useSides();
  const add = useAction<null, { scope: Scope }>("book.add_side");
  const [confirm, setConfirm] = useState<Scope | null>(null);
  const lic = useLicense();
  const missing = (["business", "household"] as Scope[]).filter((s) => !sides.sides.includes(s));
  return (
    <SettingsCard
      id="sides"
      icon={<Home size={18} />}
      title="Business & household"
      desc="One book can keep a business, a household, or both. Each side has its own categories and reports, and its numbers stay separate."
    >
      <div className="stack">
        <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
          {sides.sides.map((s) => (
            <span key={s} className="pill pill-brand">
              <Check size={12} /> {SCOPE_LABEL[s]}
            </span>
          ))}
        </div>
        {sides.both ? (
          <p className="muted small">
            When business money pays for something at home, or the other way around, Tallybook records it as an owner's draw or contribution
            for you, so both sides stay right.
          </p>
        ) : (
          missing.map((s) =>
            s === "business" && !lic.has("business") ? (
              <UpgradeOffer key={s} need="business" compact />
            ) : (
            <div key={s} className="row" style={{ gap: 12, flexWrap: "wrap" }}>
              <button className="btn btn-secondary" onClick={() => setConfirm(s)}>
                <Plus size={16} /> Add a {s} side
              </button>
              <span className="muted small">{SIDE_PITCH[s]}</span>
            </div>
            ),
          )
        )}
      </div>
      <ConfirmModal
        open={confirm !== null}
        onClose={() => setConfirm(null)}
        title={`Add a ${confirm ?? ""} side?`}
        body={
          <>
            This adds {confirm === "household" ? "household categories like Groceries, Rent and Kids' Activities" : "a business chart of accounts"} to this book.
            Nothing you've already recorded changes.
          </>
        }
        confirmLabel="Add it"
        onConfirm={async () => {
          if (!confirm) return;
          try {
            await add.mutateAsync({ scope: confirm });
            toast({ text: `${SCOPE_LABEL[confirm]} side added` });
          } catch (e) {
            toast.error(e);
          }
        }}
      />
    </SettingsCard>
  );
}
