// Book and app settings: company profile, sides of the book, license,
// invoices, importing from Ledger, fiscal year, closing the books (lock
// date), appearance, encryption, bill reminders on a calendar, cloud copies, backups, updates and deleting
// the book.

import { Building2, CalendarCheck, CalendarRange, Home, Cloud, KeyRound, DatabaseBackup, Download, FileText, Lock, Palette, ShieldCheck, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useApi } from "../api";
import { useSides } from "../book";
import { ErrorBanner, Loading, PageHeader } from "../components/ui";
import type { Settings as SettingsData } from "../types";
import "./Settings.css";
import type { FormApi } from "./settings/shared";
import BusinessCard from "./settings/BusinessCard";
import SidesCard from "./settings/SidesCard";
import LicenseCard from "./settings/LicenseCard";
import InvoiceCard from "./settings/InvoiceCard";
import LedgerCard from "./settings/LedgerCard";
import FiscalCard from "./settings/FiscalCard";
import LockCard from "./settings/LockCard";
import AppearanceCard from "./settings/AppearanceCard";
import SecurityCard from "./settings/SecurityCard";
import CloudCard from "./settings/CloudCard";
import CalendarCard from "./settings/CalendarCard";
import BackupCard from "./settings/BackupCard";
import UpdatesCard from "./settings/UpdatesCard";
import DataCard from "./settings/DataCard";
import DeleteCard from "./settings/DeleteCard";

// ------------------------------------------------------------- page

/** Settings sections, in page order. `business` / `household` mark ones shown only when the book keeps that side. */
const ALL_SECTIONS = [
  { id: "business", label: "Profile", icon: <Building2 size={16} /> },
  { id: "sides", label: "Business & household", icon: <Home size={16} /> },
  { id: "license", label: "License", icon: <KeyRound size={16} /> },
  { id: "invoices", label: "Invoices", icon: <FileText size={16} />, business: true },
  { id: "ledger", label: "Move from Ledger", icon: <Download size={16} />, household: true },
  { id: "calendar", label: "Calendar", icon: <CalendarCheck size={16} /> },
  { id: "fiscal", label: "Fiscal year", icon: <CalendarRange size={16} /> },
  { id: "lock", label: "Close the books", icon: <Lock size={16} /> },
  { id: "appearance", label: "Appearance", icon: <Palette size={16} /> },
  { id: "security", label: "Encryption", icon: <ShieldCheck size={16} /> },
  { id: "cloud", label: "Cloud copies", icon: <Cloud size={16} /> },
  { id: "backup", label: "Backup", icon: <DatabaseBackup size={16} /> },
  { id: "updates", label: "Updates", icon: <Download size={16} /> },
  { id: "data", label: "About your data", icon: <ShieldCheck size={16} /> },
  { id: "delete", label: "Delete book", icon: <Trash2 size={16} /> },
];

/** Edits keyed by pref name (or "__fsm"); absent = unchanged from the server. */
type Edits = Record<string, string>;

export default function Settings() {
  const { data, error, isLoading } = useApi<SettingsData>("settings.get");
  const sides = useSides();
  const SECTIONS = ALL_SECTIONS.filter((s) => (sides.business || !("business" in s)) && (sides.household || !("household" in s)));
  const [edits, setEdits] = useState<Edits>({});
  const [active, setActive] = useState("business");

  useEffect(() => {
    const els = SECTIONS.map((s) => document.getElementById(s.id)).filter(Boolean) as HTMLElement[];
    if (!els.length) return;
    const obs = new IntersectionObserver(
      (entries) => {
        const visible = entries.filter((e) => e.isIntersecting).sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top);
        if (visible[0]) setActive(visible[0].target.id);
      },
      { rootMargin: "0px 0px -65% 0px" },
    );
    els.forEach((el) => obs.observe(el));
    return () => obs.disconnect();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [data, sides.business]);

  if (isLoading) return <Loading />;
  if (error || !data)
    return (
      <div className="page">
        <ErrorBanner error={error} />
      </div>
    );

  const form: FormApi = {
    get: (key, fallback = "") => (key in edits ? edits[key] : (data.prefs[key] ?? fallback)),
    set: (key, value) => setEdits((e) => ({ ...e, [key]: value })),
    dirty: (keys, fallbacks = {}) => keys.some((k) => k in edits && edits[k] !== (data.prefs[k] ?? fallbacks[k] ?? "")),
    reset: (keys) =>
      setEdits((e) => {
        const next = { ...e };
        keys.forEach((k) => delete next[k]);
        return next;
      }),
  };

  return (
    <div className="page">
      <PageHeader title="Settings" subtitle={sides.business ? "Your business details, invoice defaults, and how Tallybook looks." : "Your book's details and how Tallybook looks."} />
      <div className="settings-layout">
        <nav className="settings-nav">
          {SECTIONS.map((s) => (
            <a
              key={s.id}
              href={`#${s.id}`}
              className={active === s.id ? "active" : ""}
              onClick={(e) => {
                e.preventDefault();
                document.getElementById(s.id)?.scrollIntoView({ behavior: "smooth", block: "start" });
                setActive(s.id);
              }}
            >
              {s.icon}
              {s.label}
            </a>
          ))}
        </nav>

        <div className="stack" style={{ gap: 20 }}>
          <BusinessCard form={form} />
          <SidesCard />
          <LicenseCard />
          {sides.business && <InvoiceCard form={form} />}
          {sides.household && <LedgerCard />}
          <CalendarCard />
          <FiscalCard current={data.fiscal_year_start_month} />
          <LockCard lockDate={data.lock_date} />
          <AppearanceCard />
          <SecurityCard />
          <CloudCard />
          <BackupCard />
          <UpdatesCard />
          <DataCard />
          <DeleteCard />
        </div>
      </div>
    </div>
  );
}

/**
 * Brings a household over from Ledger: pick its SQLite file,
 * see what's inside, then bring it over. Safe to run again; only new entries come in.
 */
