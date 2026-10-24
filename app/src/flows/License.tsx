import { Check, ExternalLink, KeyRound, RefreshCw } from "lucide-react";
import { useState } from "react";
import { openExternal, useAction, useApi } from "../api";
import { useToast } from "../components/ui";
import type { LicenseStatus, LicenseTier } from "../types";

const RANK: Record<LicenseTier, number> = { free: 0, personal: 1, business: 2, tax: 3 };

export type OfferKind = "personal" | "business" | "tax";

/**
 * What each yearly plan costs and unlocks; each includes the ones before it.
 * When a plan ends, what it unlocked keeps working, except bank and phone sync.
 */
export const OFFERS: Record<OfferKind, { name: string; price: string; short: string; term: string; pitch: string; perks: string[] }> = {
  personal: {
    name: "Tallybook Personal",
    price: "$20",
    short: "$20/yr",
    term: "a year",
    pitch: "Your bank accounts and cards connected for you, so transactions arrive on their own, and Tallybook on your phone.",
    perks: ["Bank and card sync, set up for you", "Tallybook on your phone", "Everything in the free household budget"],
  },
  business: {
    name: "Tallybook Business",
    price: "$49",
    short: "$49/yr",
    term: "a year",
    pitch: "Keep a business next to your household: invoices, bills and business reports, with your banks syncing and Tallybook on your phone.",
    perks: ["Invoices and bills", "Profit & loss and balance sheet", "Business and household in one book", "Bank sync and the phone app"],
  },
  tax: {
    name: "Tallybook Business + Tax",
    price: "$75",
    short: "$75/yr",
    term: "a year",
    pitch: "Everything in Business, plus tax prep built from your books for this tax year and last.",
    perks: ["Schedule C worksheet", "Quarterly estimated-tax plan", "1099 tracking", "A tax package for your accountant"],
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

/**
 * Asks Lemon Squeezy about the plan again, to pick up a renewal or a plan
 * change, and says what it found.
 */
export function useCheckPlan() {
  const toast = useToast();
  const refresh = useAction<LicenseStatus, null>("license.refresh");
  const check = async () => {
    try {
      const st = await refresh.mutateAsync(null);
      toast({ text: st.active ? `${st.name} is active` : `Your ${st.name} plan has ended. Everything keeps working except bank and phone sync.` });
    } catch (e) {
      toast.error(e);
    }
  };
  return { check, checking: refresh.isPending };
}

/** A plan that has ended or doesn't cover this year: renew it, then check again. */
export function RenewPlan(props: { note: string }) {
  const lic = useLicense();
  const { check, checking } = useCheckPlan();
  const url = lic.data?.manage_url;
  return (
    <div className="upgrade">
      <p className="small bold">{props.note}</p>
      <p className="muted small" style={{ marginTop: 4 }}>
        Renew in your Lemon Squeezy account with the same key, then check your plan here.
      </p>
      <div className="row" style={{ gap: 8, marginTop: 12, flexWrap: "wrap" }}>
        <button className="btn btn-primary btn-sm" disabled={!url} onClick={() => url && openExternal(url)}>
          Renew my plan <ExternalLink size={14} />
        </button>
        <button className="btn btn-ghost btn-sm" disabled={checking} onClick={check}>
          <RefreshCw size={14} /> {checking ? "Checking…" : "Check my plan"}
        </button>
      </div>
    </div>
  );
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
  const url = { personal: lic.data?.personal_checkout, business: lic.data?.business_checkout, tax: lic.data?.tax_checkout }[props.need];
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
          Buy for {offer.price} {offer.term} <ExternalLink size={14} />
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
