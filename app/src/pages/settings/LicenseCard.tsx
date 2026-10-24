// License card: the current license, upgrade offers and key entry.

import { Check, ExternalLink, KeyRound, RefreshCw } from "lucide-react";
import { useState } from "react";
import { openExternal, useAction } from "../../api";
import { ConfirmModal, Field, useToast } from "../../components/ui";
import { date } from "../../format";
import { LicenseKeyForm, RenewPlan, UpgradeOffer, useCheckPlan, useLicense, yearSpan } from "../../flows/License";
import { SettingsCard } from "./shared";

/** What this copy is licensed for, upgrading, and moving a key to another computer. */
export default function LicenseCard() {
  const toast = useToast();
  const lic = useLicense();
  const release = useAction<null, null>("license.deactivate");
  const { check, checking } = useCheckPlan();
  const [confirm, setConfirm] = useState(false);
  const st = lic.data;
  const paid = lic.tier !== "free";
  const ended = st?.active === false;
  return (
    <SettingsCard
      id="license"
      icon={<KeyRound size={18} />}
      title="License"
      desc="Household budgeting is free. Yearly plans add bank sync and the phone app, then a business, then tax prep. If a plan ends, everything it unlocked keeps working except bank and phone sync."
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
          {paid && st?.active && st.checked_on && <span className="muted small">· plan active as of {date(st.checked_on)}</span>}
        </div>
        {ended && <RenewPlan note="Your plan has ended. Everything keeps working except bank and phone sync." />}
        {!ended && lic.tier === "tax" && !lic.coversTaxYear(new Date().getFullYear()) && (
          <RenewPlan note={`Your plan covers tax years ${yearSpan(lic.taxYears)}. If it has renewed, check your plan to add this year.`} />
        )}
        {lic.tier === "free" && <UpgradeOffer need="personal" compact noKey />}
        {!lic.has("business") && <UpgradeOffer need="business" compact noKey />}
        {!lic.has("tax") && !paid && <UpgradeOffer need="tax" compact noKey />}
        {paid && !ended && (
          <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
            <button className="btn btn-ghost btn-sm" disabled={!st?.manage_url} onClick={() => st && openExternal(st.manage_url)}>
              Manage or change my plan <ExternalLink size={14} />
            </button>
            <button className="btn btn-ghost btn-sm" disabled={checking} onClick={check}>
              <RefreshCw size={14} /> {checking ? "Checking…" : "Check my plan"}
            </button>
          </div>
        )}
        {(!paid || ended) && (
          <Field label="Already bought one?">
            <LicenseKeyForm />
          </Field>
        )}
        {paid && (
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
        body={<>Business books stay open and readable, but adding a business side, tax prep and bank sync will need the key again.</>}
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
