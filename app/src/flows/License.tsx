import { Check, ExternalLink, KeyRound } from "lucide-react";
import { useState } from "react";
import { openExternal, useAction, useApi } from "../api";
import { useToast } from "../components/ui";
import type { LicenseStatus, LicenseTier } from "../types";

const RANK: Record<LicenseTier, number> = { free: 0, business: 1, tax: 2 };

export type OfferKind = "business" | "tax" | "renewal";

/**
 * What each purchase costs and unlocks. Business is yours for good; Business +
 * Tax covers the tax year it's bought in and the one before, and returning
 * buyers renew for the next season at a discount.
 */
export const OFFERS: Record<OfferKind, { name: string; price: string; term: string; pitch: string; perks: string[] }> = {
  business: {
    name: "Tallybook Business",
    price: "$19.99",
    term: "once, yours for good",
    pitch: "Keep a business next to your household: invoices, bills, bank feeds and business reports.",
    perks: ["Invoices and bills", "Profit & loss and balance sheet", "Business and household in one book"],
  },
  tax: {
    name: "Tallybook Business + Tax",
    price: "$49.99",
    term: "this tax year and last",
    pitch: "Everything in Business, yours for good, plus tax prep built from your books for this tax year and last.",
    perks: ["Schedule C worksheet", "Quarterly estimated-tax plan", "1099 tracking", "A tax package for your accountant"],
  },
  renewal: {
    name: "Tax prep, next season",
    price: "$29.99",
    term: "for returning customers",
    pitch: "The new tax year's tables and forms, for anyone who's bought Business + Tax before.",
    perks: ["This year's brackets and thresholds", "Schedule C, estimates and 1099s", "A tax package for your accountant"],
  },
};

/** "2025–2026", or "2026" for one year. */
export function yearSpan(years: number[]) {
  if (!years.length) return "";
  const [a, b] = [years[0], years[years.length - 1]];
  return a === b ? `${a}` : `${a}–${b}`;
}

/** This copy's license: whether it covers `need`, and which tax years. */
export function useLicense() {
  const q = useApi<LicenseStatus>("license.status");
  const tier = q.data?.tier ?? "free";
  const taxYears = q.data?.tax_years ?? [];
  return { ...q, tier, taxYears, has: (need: LicenseTier) => RANK[tier] >= RANK[need], coversTaxYear: (y: number) => taxYears.includes(y) };
}

/** Paste a key, activate it. */
export function LicenseKeyForm(props: { onDone?: () => void }) {
  const toast = useToast();
  const activate = useAction<LicenseStatus, { key: string }>("license.activate");
  const [key, setKey] = useState("");
  const submit = async () => {
    try {
      const st = await activate.mutateAsync({ key });
      toast({ text: `${st.name} unlocked — thank you!` });
      setKey("");
      props.onDone?.();
    } catch (e) {
      toast.error(e);
    }
  };
  return (
    <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
      <input
        className="input grow"
        style={{ minWidth: 240, fontFamily: "var(--mono)" }}
        value={key}
        onChange={(e) => setKey(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && key.trim() && submit()}
        placeholder="Paste your license key"
        aria-label="License key"
        spellCheck={false}
      />
      <button className="btn btn-primary" disabled={!key.trim() || activate.isPending} onClick={submit}>
        <KeyRound size={16} /> {activate.isPending ? "Checking…" : "Activate"}
      </button>
    </div>
  );
}

/**
 * What `need` unlocks, a buy button, and a place to paste the key afterwards.
 * `noKey` drops the "I have a key" link where a key field is already on screen;
 * `note` says why the offer is showing.
 */
export function UpgradeOffer(props: { need: OfferKind; compact?: boolean; noKey?: boolean; note?: string }) {
  const lic = useLicense();
  const [entering, setEntering] = useState(false);
  const offer = OFFERS[props.need];
  const url = { business: lic.data?.business_checkout, tax: lic.data?.tax_checkout, renewal: lic.data?.tax_renewal_checkout }[props.need];
  return (
    <div className="upgrade">
      {props.note && <p className="small bold" style={{ marginBottom: 10 }}>{props.note}</p>}
      <div className="row" style={{ justifyContent: "space-between", alignItems: "baseline", gap: 12, flexWrap: "wrap" }}>
        <div className="bold">{offer.name}</div>
        <div>
          <span className="upgrade-price">{offer.price}</span> <span className="muted small">{offer.term}</span>
        </div>
      </div>
      <p className="muted small" style={{ marginTop: 4 }}>
        {offer.pitch}
      </p>
      {!props.compact && (
        <ul className="upgrade-perks">
          {offer.perks.map((p) => (
            <li key={p}>
              <Check size={14} className="pos" /> {p}
            </li>
          ))}
        </ul>
      )}
      <div className="row" style={{ gap: 8, marginTop: 12, flexWrap: "wrap" }}>
        <button className="btn btn-primary btn-sm" disabled={!url} onClick={() => url && openExternal(url)}>
          Buy for {offer.price} <ExternalLink size={14} />
        </button>
        {!entering && !props.noKey && (
          <button className="btn btn-ghost btn-sm" onClick={() => setEntering(true)}>
            I have a key
          </button>
        )}
      </div>
      {entering && (
        <div style={{ marginTop: 12 }}>
          <LicenseKeyForm />
        </div>
      )}
    </div>
  );
}
