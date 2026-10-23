// License card: the current license, upgrade offers and key entry.

import { Check, KeyRound } from "lucide-react";
import { useState } from "react";
import { useAction } from "../../api";
import { ConfirmModal, Field, useToast } from "../../components/ui";
import { date } from "../../format";
import { LicenseKeyForm, UpgradeOffer, useLicense, yearSpan } from "../../flows/License";
import { SettingsCard } from "./shared";

/** What this copy is licensed for, upgrading, and moving a key to another computer. */
export default function LicenseCard() {
  const toast = useToast();
  const lic = useLicense();
  const release = useAction<null, null>("license.deactivate");
  const [confirm, setConfirm] = useState(false);
  const st = lic.data;
  return (
    <SettingsCard
      id="license"
      icon={<KeyRound size={18} />}
      title="License"
      desc="Household books are free. Business is yours for good; tax prep covers the tax year you buy it in and the one before. Keys are checked once online, then kept on this computer."
    >
      <div className="stack">
        <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
          <span className={`pill ${lic.tier === "free" ? "" : "pill-brand"}`}>
            {lic.tier !== "free" && <Check size={12} />} {st?.name ?? "…"}
          </span>
          {st?.key_hint && <span className="muted small">Key ending {st.key_hint}</span>}
          {st?.email && <span className="muted small">· {st.email}</span>}
          {st?.activated_on && <span className="muted small">· since {date(st.activated_on)}</span>}
          {lic.taxYears.length > 0 && <span className="pill">Tax years {yearSpan(lic.taxYears)}</span>}
        </div>
        {lic.tier === "free" && <UpgradeOffer need="business" compact noKey />}
        {lic.tier !== "tax" && <UpgradeOffer need="tax" compact noKey />}
        {lic.tier === "tax" && !lic.coversTaxYear(new Date().getFullYear()) && (
          <UpgradeOffer need="renewal" compact noKey note={`Your license covers ${yearSpan(lic.taxYears)}. Renew for this tax season.`} />
        )}
        {(lic.tier !== "tax" || !lic.coversTaxYear(new Date().getFullYear())) && (
          <Field label="Already bought one?">
            <LicenseKeyForm />
          </Field>
        )}
        {lic.tier !== "free" && (
          <div className="row" style={{ gap: 12, flexWrap: "wrap" }}>
            <button className="btn btn-ghost btn-sm" onClick={() => setConfirm(true)}>
              Remove from this computer
            </button>
            <span className="muted small">Frees the key so you can activate it on another computer. Your books aren't touched.</span>
          </div>
        )}
      </div>
      <ConfirmModal
        open={confirm}
        onClose={() => setConfirm(false)}
        title="Remove the license from this computer?"
        body={<>Business books stay open and readable, but adding a business side and tax prep will need the key again.</>}
        confirmLabel="Remove it"
        onConfirm={async () => {
          try {
            await release.mutateAsync(null);
            toast({ text: "License removed from this computer" });
          } catch (e) {
            toast.error(e);
          }
        }}
      />
    </SettingsCard>
  );
}
