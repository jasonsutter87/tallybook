import { Building2, CalendarRange, Home, Plus, Check, Cloud, Copy, GitBranch, KeyRound, ShieldOff, DatabaseBackup, Download, FileText, Lock, LockOpen, Monitor, Moon, Palette, ShieldCheck, Sun, Trash2 } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { call, isTauri, queryClient, useAction, useApi } from "../api";
import { SCOPE_LABEL, useSides } from "../book";
import { ConfirmModal, ErrorBanner, Field, Loading, Modal, PageHeader, useToast } from "../components/ui";
import { date } from "../format";
import type { BookStatus, CloudBackupReport, CloudStatus, EncryptResult, ISODate, LedgerImport, LedgerPreview, Scope, Settings as SettingsData } from "../types";
import "./Settings.css";
import { readTheme, saveTheme, type ThemeChoice } from "../theme";
import { currentVersion, useUpdater } from "../updater";
import DeleteBookModal from "../flows/DeleteBookModal";

// ------------------------------------------------------------- page

const ALL_SECTIONS = [
  { id: "business", label: "Profile", icon: <Building2 size={16} /> },
  { id: "sides", label: "Business & household", icon: <Home size={16} /> },
  { id: "invoices", label: "Invoices", icon: <FileText size={16} />, business: true },
  { id: "ledger", label: "Move from Ledger", icon: <Download size={16} />, household: true },
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

const MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

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
          {sides.business && <InvoiceCard form={form} />}
          {sides.household && <LedgerCard />}
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

interface FormApi {
  get: (key: string, fallback?: string) => string;
  set: (key: string, value: string) => void;
  dirty: (keys: string[], fallbacks?: Record<string, string>) => boolean;
  reset: (keys: string[]) => void;
}

function SettingsCard(props: { id: string; icon: ReactNode; title: string; desc: ReactNode; children: ReactNode; foot?: ReactNode }) {
  return (
    <section className="card settings-card" id={props.id}>
      <div className="card-header">
        <div className="settings-card-head">
          <span className="icon-bubble">{props.icon}</span>
          <div>
            <h3 className="card-title">{props.title}</h3>
            <div className="settings-card-desc">{props.desc}</div>
          </div>
        </div>
      </div>
      <div className="card-body" style={{ paddingTop: 20 }}>
        {props.children}
      </div>
      {props.foot && <div className="settings-foot">{props.foot}</div>}
    </section>
  );
}

/** Save/Discard footer shared by the pref cards. */
function SaveFoot(props: { dirty: boolean; busy: boolean; onSave: () => void; onDiscard: () => void }) {
  return (
    <>
      {props.dirty ? <span className="subtle small grow">You have unsaved changes</span> : <span className="subtle small grow">All changes saved</span>}
      {props.dirty && (
        <button className="btn btn-ghost btn-sm" onClick={props.onDiscard} disabled={props.busy}>
          Discard
        </button>
      )}
      <button className="btn btn-primary btn-sm" onClick={props.onSave} disabled={!props.dirty || props.busy}>
        {props.busy ? "Saving…" : "Save changes"}
      </button>
    </>
  );
}

function useSavePrefs(form: FormApi, keys: string[], label: string) {
  const toast = useToast();
  const save = useAction<void, unknown>("settings.set");
  const run = async (prefs: Record<string, string | null>) => {
    try {
      await save.mutateAsync({ prefs });
      form.reset(keys);
      toast({ text: `${label} saved` });
    } catch (e) {
      toast.error(e);
    }
  };
  return { run, busy: save.isPending };
}

// ------------------------------------------------------------- business

const BUSINESS_KEYS = ["company.name", "company.email", "company.phone", "company.address"];

function BusinessCard({ form }: { form: FormApi }) {
  const { run, busy } = useSavePrefs(form, BUSINESS_KEYS, "Business profile");
  const dirty = form.dirty(BUSINESS_KEYS);
  const nameMissing = form.get("company.name").trim() === "";
  return (
    <SettingsCard
      id="business"
      icon={<Building2 size={18} />}
      title="Business profile"
      desc="Shown on your invoices and at the top of printed reports."
      foot={
        <SaveFoot
          dirty={dirty}
          busy={busy}
          onDiscard={() => form.reset(BUSINESS_KEYS)}
          onSave={() => {
            if (nameMissing) return;
            run(Object.fromEntries(BUSINESS_KEYS.map((k) => [k, form.get(k).trim() || null])));
          }}
        />
      }
    >
      <div className="form-grid">
        <Field label="Business name" className="span-2" hint={nameMissing ? <span className="neg">Your business needs a name.</span> : undefined}>
          <input className="input" value={form.get("company.name")} onChange={(e) => form.set("company.name", e.target.value)} placeholder="Northwind Studio" />
        </Field>
        <Field label="Email">
          <input className="input" type="email" value={form.get("company.email")} onChange={(e) => form.set("company.email", e.target.value)} placeholder="hello@yourbusiness.com" />
        </Field>
        <Field label="Phone">
          <input className="input" type="tel" value={form.get("company.phone")} onChange={(e) => form.set("company.phone", e.target.value)} placeholder="(555) 123-4567" />
        </Field>
        <Field label="Address" className="span-2">
          <textarea className="textarea" rows={3} value={form.get("company.address")} onChange={(e) => form.set("company.address", e.target.value)} placeholder={"123 Main St\nCity, ST 00000"} />
        </Field>
      </div>
    </SettingsCard>
  );
}

// ------------------------------------------------------------- invoices

const INVOICE_KEYS = ["invoice.next_number", "invoice.default_terms_days", "invoice.default_tax_bp", "invoice.footer"];
const INVOICE_DEFAULTS: Record<string, string> = { "invoice.default_terms_days": "30" };
const TAX_TEXT = "__tax_pct";

function bpToPct(bp: string): string {
  const n = Number(bp);
  if (!bp || !Number.isFinite(n)) return "";
  return String(n / 100);
}

/** "8.25" → "825"; "" → ""; invalid → null. */
function pctToBp(pct: string): string | null {
  const s = pct.trim().replace(/%$/, "").trim();
  if (s === "") return "";
  if (!/^\d{0,3}(\.\d{0,3})?$/.test(s) || s === ".") return null;
  const n = Number(s);
  if (!Number.isFinite(n) || n > 100) return null;
  return String(Math.round(n * 100));
}

function InvoiceCard({ form }: { form: FormApi }) {
  const keys = [...INVOICE_KEYS, TAX_TEXT];
  const { run, busy } = useSavePrefs(form, keys, "Invoice settings");
  const taxText = form.get(TAX_TEXT, bpToPct(form.get("invoice.default_tax_bp")));
  const taxBp = pctToBp(taxText);
  const taxDirty = taxBp !== null && taxBp !== form.get("invoice.default_tax_bp");
  const dirty = form.dirty(INVOICE_KEYS, INVOICE_DEFAULTS) || taxDirty || taxBp === null;
  const nextNumber = form.get("invoice.next_number");

  return (
    <SettingsCard
      id="invoices"
      icon={<FileText size={18} />}
      title="Invoices"
      desc="Defaults for new invoices. You can still change any of these on a single invoice."
      foot={
        <SaveFoot
          dirty={dirty}
          busy={busy}
          onDiscard={() => form.reset(keys)}
          onSave={() => {
            if (taxBp === null) return;
            run({
              "invoice.next_number": nextNumber.trim() || null,
              "invoice.default_terms_days": form.get("invoice.default_terms_days", "30"),
              "invoice.default_tax_bp": taxBp || null,
              "invoice.footer": form.get("invoice.footer").trim() || null,
            });
          }}
        />
      }
    >
      <div className="form-grid">
        <Field label="Next invoice number" hint="The number your next invoice will get. Later ones count up from here.">
          <input className="input" value={nextNumber} onChange={(e) => form.set("invoice.next_number", e.target.value)} placeholder="1001" inputMode="numeric" />
        </Field>
        <Field label="Payment terms" hint="How long customers have to pay.">
          <select className="select" value={form.get("invoice.default_terms_days", "30")} onChange={(e) => form.set("invoice.default_terms_days", e.target.value)}>
            <option value="0">Due on receipt</option>
            <option value="15">Net 15 — due in 15 days</option>
            <option value="30">Net 30 — due in 30 days</option>
            <option value="60">Net 60 — due in 60 days</option>
          </select>
        </Field>
        <Field
          label="Default sales tax"
          hint={taxBp === null ? <span className="neg">Enter a percentage like 8.25</span> : "Applied to taxable lines. Leave blank if you don't charge sales tax."}
        >
          <div style={{ position: "relative" }}>
            <input className="input num" style={{ paddingRight: 32 }} value={taxText} onChange={(e) => form.set(TAX_TEXT, e.target.value)} placeholder="0" inputMode="decimal" />
            <span className="subtle bold" style={{ position: "absolute", right: 12, top: "50%", transform: "translateY(-50%)" }}>
              %
            </span>
          </div>
        </Field>
        <div />
        <Field label="Footer message" className="span-2" hint="Printed at the bottom of every invoice — a thank-you, payment instructions, etc.">
          <textarea className="textarea" rows={2} value={form.get("invoice.footer")} onChange={(e) => form.set("invoice.footer", e.target.value)} placeholder="Thank you for your business!" />
        </Field>
      </div>
    </SettingsCard>
  );
}

// ------------------------------------------------------------- fiscal year

function FiscalCard({ current }: { current: number }) {
  const toast = useToast();
  const save = useAction<void, unknown>("settings.set");
  const [month, setMonth] = useState(current);
  useEffect(() => setMonth(current), [current]);
  const dirty = month !== current;
  return (
    <SettingsCard
      id="fiscal"
      icon={<CalendarRange size={18} />}
      title="Fiscal year"
      desc="When your business year starts. Most small businesses use January."
      foot={
        <SaveFoot
          dirty={dirty}
          busy={save.isPending}
          onDiscard={() => setMonth(current)}
          onSave={async () => {
            try {
              await save.mutateAsync({ fiscal_year_start_month: month });
              toast({ text: "Fiscal year saved" });
            } catch (e) {
              toast.error(e);
            }
          }}
        />
      }
    >
      <div className="form-grid">
        <Field label="First month of the year" hint={`"Year to date" reports and "Net income (this year)" on the balance sheet count from the 1st of ${MONTHS[month - 1]}.`}>
          <select className="select" value={month} onChange={(e) => setMonth(Number(e.target.value))}>
            {MONTHS.map((m, i) => (
              <option key={m} value={i + 1}>
                {m}
              </option>
            ))}
          </select>
        </Field>
      </div>
    </SettingsCard>
  );
}

// ------------------------------------------------------------- lock date

function LockCard({ lockDate }: { lockDate: ISODate | null }) {
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

// ------------------------------------------------------------- appearance

function AppearanceCard() {
  const [theme, setTheme] = useState<ThemeChoice>(readTheme);
  const choices: { value: ThemeChoice; label: string; icon: ReactNode; swatch: [string, string][] }[] = [
    {
      value: "system",
      label: "System",
      icon: <Monitor size={15} />,
      swatch: [
        ["#f3f5f9", "#d5dbe6"],
        ["#0a1220", "#2a3a54"],
      ],
    },
    { value: "light", label: "Light", icon: <Sun size={15} />, swatch: [["#f3f5f9", "#d5dbe6"]] },
    { value: "dark", label: "Dark", icon: <Moon size={15} />, swatch: [["#0a1220", "#2a3a54"]] },
  ];
  return (
    <SettingsCard id="appearance" icon={<Palette size={18} />} title="Appearance" desc="Saved on this computer. System follows your device's light or dark setting.">
      <div className="theme-choices" role="radiogroup" aria-label="Theme">
        {choices.map((c) => (
          <button
            key={c.value}
            role="radio"
            aria-checked={theme === c.value}
            className={`theme-choice ${theme === c.value ? "active" : ""}`}
            onClick={() => {
              setTheme(c.value);
              saveTheme(c.value);
            }}
          >
            <div className="theme-swatch">
              {c.swatch.map(([bg, bar], i) => (
                <div key={i} style={{ background: bg }}>
                  <i style={{ background: "#0aa2c0", width: "45%" }} />
                  <i style={{ background: bar, width: "80%" }} />
                  <i style={{ background: bar, width: "60%" }} />
                </div>
              ))}
            </div>
            <span className="row-between">
              <span className="row" style={{ gap: 6 }}>
                {c.icon} {c.label}
              </span>
              {theme === c.value && <Check size={15} style={{ color: "var(--brand)" }} />}
            </span>
          </button>
        ))}
      </div>
    </SettingsCard>
  );
}

// ------------------------------------------------------------- updates

function UpdatesCard() {
  const { state, check, install } = useUpdater(false);
  const [version, setVersion] = useState<string | null>(null);
  useEffect(() => {
    currentVersion().then(setVersion, () => setVersion(null));
  }, []);
  return (
    <SettingsCard
      id="updates"
      icon={<Download size={18} />}
      title="Updates"
      desc="Tallybook checks for new versions a few times a day. Every update is signed, and your book is backed up before it installs."
    >
      <div className="stack">
        <div className="row" style={{ gap: 12, flexWrap: "wrap" }}>
          <span className="pill">{version ? `Version ${version}` : "Browser preview"}</span>
          {state.kind === "available" ? (
            <button className="btn btn-primary" onClick={install}>
              <Download size={16} /> Update to {state.version}
            </button>
          ) : (
            <button className="btn btn-secondary" disabled={state.kind === "checking" || state.kind === "downloading" || state.kind === "installing"} onClick={() => check(true)}>
              {state.kind === "checking" ? "Checking…" : "Check for updates"}
            </button>
          )}
          {state.kind === "none" && <span className="muted small">You're on the latest version.</span>}
          {state.kind === "downloading" && <span className="muted small">Downloading{state.percent != null ? ` · ${state.percent}%` : "…"}</span>}
          {state.kind === "installing" && <span className="muted small">Installing — Tallybook will restart.</span>}
        </div>
        {state.kind === "error" && <div className="banner error">{state.message}</div>}
      </div>
    </SettingsCard>
  );
}

// ------------------------------------------------------------- backup & data

const SIDE_PITCH: Record<Scope, string> = {
  business: "Invoices, bills, business reports and tax prep, kept apart from the household's numbers.",
  household: "A monthly budget, bills coming up, and goals, kept apart from the business's numbers.",
};

/** Which sides this book keeps, and adding the other one. */
function SidesCard() {
  const toast = useToast();
  const sides = useSides();
  const add = useAction<null, { scope: Scope }>("book.add_side");
  const [confirm, setConfirm] = useState<Scope | null>(null);
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
          missing.map((s) => (
            <div key={s} className="row" style={{ gap: 12, flexWrap: "wrap" }}>
              <button className="btn btn-secondary" onClick={() => setConfirm(s)}>
                <Plus size={16} /> Add a {s} side
              </button>
              <span className="muted small">{SIDE_PITCH[s]}</span>
            </div>
          ))
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

function BackupCard() {
  const toast = useToast();
  const backup = useAction<string, unknown>("book.backup");
  const [last, setLast] = useState<string | null>(null);
  return (
    <SettingsCard
      id="backup"
      icon={<DatabaseBackup size={18} />}
      title="Backup"
      desc="A backup is a full copy of your book file, saved in a Backups folder right next to it."
    >
      <div className="stack">
        <div className="row" style={{ gap: 12, flexWrap: "wrap" }}>
          <button
            className="btn btn-primary"
            disabled={backup.isPending}
            onClick={async () => {
              try {
                const path = await backup.mutateAsync(null);
                setLast(path);
                toast({ text: `Backed up to ${path}` });
              } catch (e) {
                toast.error(e);
              }
            }}
          >
            <DatabaseBackup size={16} /> {backup.isPending ? "Backing up…" : "Back up now"}
          </button>
          <span className="muted small">Takes a second. Each backup is dated, so older ones are never overwritten.</span>
        </div>
        {last && (
          <div>
            <div className="label" style={{ marginBottom: 6 }}>
              Latest backup
            </div>
            <div className="path-box">{last}</div>
          </div>
        )}
        <div className="hint">To restore, open a backup file from the Welcome screen like any other book. Copying your book file to a USB drive or cloud folder works too.</div>
      </div>
    </SettingsCard>
  );
}

function DataCard() {
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
              <b>Open format.</b> <span className="muted">The book is a standard SQLite database — readable by many tools, today and decades from now.</span>
            </span>
          </li>
        </ul>
      </div>
    </SettingsCard>
  );
}

// ------------------------------------------------------------- delete

function DeleteCard() {
  const status = useApi<BookStatus>("book.status");
  const [open, setOpen] = useState(false);
  const path = status.data?.path;
  const name = status.data?.company_name ?? "";
  return (
    <SettingsCard
      id="delete"
      icon={<Trash2 size={18} />}
      title="Delete this book"
      desc="Closes the book, disconnects its banks, removes their keys from your keychain, and moves the file to the Trash."
    >
      <button className="btn btn-danger" disabled={!path} onClick={() => setOpen(true)}>
        <Trash2 size={16} /> Delete {name || "book"}…
      </button>
      {path && <DeleteBookModal open={open} onClose={() => setOpen(false)} path={path} name={name} />}
    </SettingsCard>
  );
}

// ------------------------------------------------------------- encryption

/** Shows a recovery key with copy, and a "saved it" acknowledgement. */
function RecoveryKeyView(props: { recoveryKey: string }) {
  const toast = useToast();
  return (
    <div className="stack-sm">
      <div
        style={{
          fontFamily: "var(--mono)",
          fontSize: 15,
          lineHeight: 1.7,
          letterSpacing: ".04em",
          padding: "14px 16px",
          borderRadius: 12,
          background: "var(--surface-2)",
          border: "1px dashed var(--line-strong)",
          wordBreak: "break-all",
          userSelect: "all",
        }}
      >
        {props.recoveryKey}
      </div>
      <button
        className="btn btn-secondary btn-sm"
        style={{ alignSelf: "flex-start" }}
        onClick={async () => {
          try {
            await navigator.clipboard.writeText(props.recoveryKey);
            toast({ text: "Recovery key copied. Paste it into your password manager." });
          } catch (e) {
            toast.error(e);
          }
        }}
      >
        <Copy size={14} /> Copy
      </button>
    </div>
  );
}

function SecurityCard() {
  const toast = useToast();
  const status = useApi<BookStatus>("book.status");
  const encrypt = useAction<EncryptResult>("book.encrypt");
  const decrypt = useAction("book.decrypt");
  const dropPlain = useAction<number>("book.delete_plain_backups");
  const [confirming, setConfirming] = useState(false);
  const [result, setResult] = useState<EncryptResult | null>(null);
  const [saved, setSaved] = useState(false);
  const [shown, setShown] = useState<string | null>(null);
  const [turningOff, setTurningOff] = useState(false);
  const encrypted = !!status.data?.encrypted;

  return (
    <SettingsCard
      id="security"
      icon={<ShieldCheck size={18} />}
      title="Encryption"
      desc="Lock the whole book file with a random 256-bit key. Without the key the file is unreadable, whether it's on a lost laptop, in a backup, or in the cloud."
    >
      {encrypted ? (
        <div className="stack">
          <div className="banner" style={{ background: "var(--pos-soft)", color: "var(--pos)" }}>
            <ShieldCheck size={18} /> This book is encrypted. It opens automatically on this computer because its key is in your keychain.
          </div>
          <div className="row" style={{ gap: 10, flexWrap: "wrap" }}>
            <button
              className="btn btn-secondary"
              onClick={async () => {
                try {
                  setShown(await call<string>("book.recovery_key"));
                } catch (e) {
                  toast.error(e);
                }
              }}
            >
              <KeyRound size={16} /> Show recovery key
            </button>
            <button className="btn btn-ghost" onClick={() => setTurningOff(true)}>
              <ShieldOff size={16} /> Turn off encryption
            </button>
          </div>
          {shown && (
            <div className="stack-sm">
              <div className="label">Recovery key</div>
              <RecoveryKeyView recoveryKey={shown} />
              <button className="btn btn-ghost btn-sm" style={{ alignSelf: "flex-start" }} onClick={() => setShown(null)}>
                Hide
              </button>
            </div>
          )}
        </div>
      ) : (
        <div className="stack">
          <ul className="muted" style={{ margin: 0, paddingLeft: 20, lineHeight: 1.7 }}>
            <li>Day to day nothing changes: the key is kept in your system keychain.</li>
            <li>You get a <b>recovery key</b> to open the book on another computer or restore a cloud copy. Keep it somewhere safe, like a password manager.</li>
            <li>Backups and cloud copies made from now on are encrypted too.</li>
          </ul>
          <button className="btn btn-primary" style={{ alignSelf: "flex-start" }} onClick={() => setConfirming(true)}>
            <ShieldCheck size={16} /> Encrypt this book
          </button>
        </div>
      )}

      <Modal
        open={confirming}
        onClose={() => {
          if (!result || saved) {
            setConfirming(false);
            setResult(null);
            setSaved(false);
          }
        }}
        title={result ? "Save your recovery key" : "Encrypt this book?"}
        footer={
          result ? (
            <button
              className="btn btn-primary"
              disabled={!saved}
              onClick={() => {
                setConfirming(false);
                setResult(null);
                setSaved(false);
              }}
            >
              Done
            </button>
          ) : (
            <>
              <button className="btn btn-secondary" onClick={() => setConfirming(false)}>
                Cancel
              </button>
              <button
                className="btn btn-primary"
                disabled={encrypt.isPending}
                onClick={async () => {
                  try {
                    setResult(await encrypt.mutateAsync(null));
                  } catch (e) {
                    toast.error(e);
                  }
                }}
              >
                {encrypt.isPending ? "Encrypting…" : "Encrypt"}
              </button>
            </>
          )
        }
      >
        {result ? (
          <div className="stack">
            <div className="muted">
              This is the <b>only</b> way to open this book on another computer, or to restore it if this computer's keychain is lost. Tallybook can't recover it for you.
            </div>
            <RecoveryKeyView recoveryKey={result.recovery_key} />
            {result.plain_backups > 0 && (
              <div className="banner warn" style={{ alignItems: "flex-start" }}>
                <span style={{ fontWeight: 500 }}>
                  {result.plain_backups === 1 ? "1 backup" : `${result.plain_backups} backups`} from before encryption can still be read by anyone with the file.{" "}
                  <button
                    className="btn btn-secondary btn-sm"
                    style={{ marginTop: 8 }}
                    onClick={async () => {
                      try {
                        const n = await dropPlain.mutateAsync(null);
                        setResult({ ...result, plain_backups: 0 });
                        toast({ text: `Deleted ${n} unencrypted ${n === 1 ? "backup" : "backups"}` });
                      } catch (e) {
                        toast.error(e);
                      }
                    }}
                  >
                    Delete them permanently
                  </button>
                </span>
              </div>
            )}
            <label className="row" style={{ gap: 10, cursor: "pointer", fontWeight: 600 }}>
              <input type="checkbox" className="checkbox" checked={saved} onChange={(e) => setSaved(e.target.checked)} />
              I've saved my recovery key somewhere safe
            </label>
          </div>
        ) : (
          <div className="muted">
            Tallybook will make an encrypted copy, check that it matches your book exactly, and then replace the original. It takes a moment. You'll see your recovery key next.
          </div>
        )}
      </Modal>

      <ConfirmModal
        open={turningOff}
        onClose={() => setTurningOff(false)}
        title="Turn off encryption?"
        body="The book file will be stored unencrypted again. Existing encrypted backups and cloud copies stay encrypted and still need the recovery key."
        confirmLabel="Turn off encryption"
        danger
        onConfirm={async () => {
          try {
            await decrypt.mutateAsync(null);
            toast({ text: "Encryption turned off" });
          } catch (e) {
            toast.error(e);
          }
        }}
      />
    </SettingsCard>
  );
}

// ------------------------------------------------------------- moving from Ledger

async function pickLedgerFile(): Promise<string | null> {
  if (!isTauri) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const r = await open({ multiple: false, filters: [{ name: "Ledger database", extensions: ["db", "sqlite", "sqlite3"] }] });
  return typeof r === "string" ? r : null;
}

/** Brings a household over from Ledger (mels-budget-app). Safe to run again. */
function LedgerCard() {
  const toast = useToast();
  const [path, setPath] = useState("");
  const [preview, setPreview] = useState<LedgerPreview | null>(null);
  const [result, setResult] = useState<LedgerImport | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<Error | null>(null);

  const look = async (p: string) => {
    setError(null);
    setResult(null);
    setPreview(null);
    try {
      setPreview(await call<LedgerPreview>("import.ledger_preview", { path: p }));
    } catch (e) {
      setError(e as Error);
    }
  };
  const bringOver = async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await call<LedgerImport>("import.ledger", { path });
      setResult(r);
      await queryClient.invalidateQueries();
      toast({ text: `${r.entries} entries brought over from Ledger` });
    } catch (e) {
      setError(e as Error);
    } finally {
      setBusy(false);
    }
  };

  return (
    <SettingsCard
      id="ledger"
      icon={<Download size={18} />}
      title="Move from Ledger"
      desc="Bring people, entries, monthly plans, bills, goals and stores over from the Ledger budgeting app. Ledger's file is only read, never changed, and running this again adds only what's new."
    >
      <div className="stack">
        <ErrorBanner error={error} />
        <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
          {isTauri ? (
            <button
              className="btn btn-secondary"
              onClick={async () => {
                const p = await pickLedgerFile();
                if (p) {
                  setPath(p);
                  await look(p);
                }
              }}
            >
              Choose Ledger's database file…
            </button>
          ) : (
            <>
              <input className="input grow" value={path} onChange={(e) => setPath(e.target.value)} placeholder="Path to ledger.db" />
              <button className="btn btn-secondary" disabled={!path.trim()} onClick={() => look(path.trim())}>
                Look inside
              </button>
            </>
          )}
        </div>
        <p className="muted small">
          On a self-hosted Ledger it's <code>data/ledger.db</code>. On Turso, make a copy first with <code>turso db shell &lt;database&gt; .dump</code> and
          load it into a file with <code>sqlite3 ledger.db &lt; dump.sql</code>.
        </p>
        {preview && !result && (
          <div className="banner" style={{ display: "block" }}>
            {preview.entries} entries{preview.first && preview.last ? ` from ${date(preview.first)} to ${date(preview.last)}` : ""}, {preview.people} people,{" "}
            {preview.months} months of plans, {preview.bills} bills, {preview.goals} goals and {preview.stores} stores.
            <div style={{ marginTop: 10 }}>
              <button className="btn btn-primary" disabled={busy} onClick={bringOver}>
                {busy ? "Bringing it over…" : "Bring it over"}
              </button>
            </div>
          </div>
        )}
        {result && (
          <div className="stack-sm">
            <div className="bold">
              Brought over {result.entries} entries{result.entries_skipped ? ` (${result.entries_skipped} were already here)` : ""}, {result.people} people,{" "}
              {result.months} months of plans, {result.bills} bills, {result.goals} goals and {result.rules} rules.
            </div>
            <div className="muted small">
              Ledger didn't know which bank paid, so the money moved through an account called "Ledger import". Every entry is tagged with who it was
              for and what it was, so this month's plan and reports work straight away.
            </div>
            {result.notes.map((n, i) => (
              <div key={i} className="muted small">
                {n}
              </div>
            ))}
          </div>
        )}
      </div>
    </SettingsCard>
  );
}

// ------------------------------------------------------------- cloud copies

async function pickFolder(): Promise<string | null> {
  if (!isTauri) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const r = await open({ directory: true, multiple: false });
  return typeof r === "string" ? r : null;
}

function CloudCard() {
  const toast = useToast();
  const status = useApi<CloudStatus>("cloud.status");
  const set = useAction("cloud.set");
  const backup = useAction<CloudBackupReport>("cloud.backup_now");
  const [typedFolder, setTypedFolder] = useState("");
  const [typedRepo, setTypedRepo] = useState("");
  const s = status.data;

  const save = async (patch: Record<string, unknown>) => {
    try {
      await set.mutateAsync(patch);
    } catch (e) {
      toast.error(e);
    }
  };

  const destination = (kind: "folder" | "git", value: string | null) => {
    const isGit = kind === "git";
    const typed = isGit ? typedRepo : typedFolder;
    const setTyped = isGit ? setTypedRepo : setTypedFolder;
    return (
      <div className="stack-sm">
        <div className="label row" style={{ gap: 6 }}>
          {isGit ? <GitBranch size={14} /> : <Cloud size={14} />}
          {isGit ? "Git repository" : "Synced folder"}
        </div>
        {value ? (
          <div className="row" style={{ gap: 8 }}>
            <div className="path-box grow" style={{ margin: 0 }}>
              {value}
            </div>
            <button className="btn btn-ghost btn-sm" onClick={() => save({ [kind]: null })}>
              Remove
            </button>
          </div>
        ) : (
          <>
            {!isGit && !!s?.suggestions.length && (
              <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
                {s.suggestions.map((g) => (
                  <button key={g.path} className="btn btn-secondary btn-sm" onClick={() => save({ folder: g.path })}>
                    <Cloud size={13} /> {g.label}
                  </button>
                ))}
              </div>
            )}
            <div className="row" style={{ gap: 8 }}>
              {isTauri ? (
                <button
                  className="btn btn-secondary btn-sm"
                  onClick={async () => {
                    const p = await pickFolder();
                    if (p) await save({ [kind]: p });
                  }}
                >
                  Choose {isGit ? "repository" : "folder"}…
                </button>
              ) : (
                <>
                  <input className="input grow" placeholder={isGit ? "/path/to/cloned/repo" : "/path/to/synced/folder"} value={typed} onChange={(e) => setTyped(e.target.value)} />
                  <button className="btn btn-secondary btn-sm" disabled={!typed.trim()} onClick={() => save({ [kind]: typed.trim() })}>
                    Use
                  </button>
                </>
              )}
            </div>
            <div className="hint">
              {isGit
                ? "A repository you've already cloned (ideally private). Tallybook commits the encrypted book and pushes with your own git login."
                : "Pick your Google Drive, iCloud Drive, Dropbox or OneDrive folder. Their desktop apps upload what Tallybook saves there."}
            </div>
          </>
        )}
        {isGit && value && !s?.git_has_remote && <div className="hint">This repository has no remote yet, so copies are committed locally only.</div>}
      </div>
    );
  };

  return (
    <SettingsCard
      id="cloud"
      icon={<Cloud size={18} />}
      title="Cloud copies"
      desc="Keep encrypted copies off this computer. Only encrypted books can be copied, so nothing readable ever leaves your machine."
    >
      {!s ? (
        <Loading />
      ) : !s.encrypted ? (
        <div className="banner warn">
          <ShieldCheck size={18} /> Turn on encryption above first. Cloud copies are always encrypted.
        </div>
      ) : (
        <div className="stack" style={{ gap: 20 }}>
          {destination("folder", s.folder)}
          {destination("git", s.git)}
          <div className="divider" style={{ margin: 0 }} />
          <div className="row" style={{ gap: 16, flexWrap: "wrap" }}>
            <label className="switch">
              <input type="checkbox" checked={s.auto} onChange={(e) => save({ auto: e.target.checked })} />
              Copy automatically once a day
            </label>
            <label className="row small muted" style={{ gap: 8 }}>
              Keep the last
              <select className="select" style={{ width: 80, height: 32 }} value={s.keep} onChange={(e) => save({ keep: Number(e.target.value) })}>
                {[3, 5, 10, 20, 50].map((n) => (
                  <option key={n} value={n}>
                    {n}
                  </option>
                ))}
              </select>
              copies in the folder
            </label>
          </div>
          <div className="row" style={{ gap: 12, flexWrap: "wrap" }}>
            <button
              className="btn btn-primary"
              disabled={backup.isPending || (!s.folder && !s.git)}
              onClick={async () => {
                try {
                  const r = await backup.mutateAsync(null);
                  for (const [label, d] of [["Folder", r.folder], ["Git", r.git]] as const) {
                    if (!d) continue;
                    if (d.ok) toast({ text: `${label}: copied${d.detail ? `. ${d.detail}` : ""}` });
                    else toast.error(`${label}: ${d.detail}`);
                  }
                } catch (e) {
                  toast.error(e);
                }
              }}
            >
              <Cloud size={16} /> {backup.isPending ? "Copying…" : "Copy now"}
            </button>
            <span className="muted small">
              {s.last ? `Last copy ${new Date(s.last).toLocaleString()}` : "No copies yet"}
              {s.last_error && <span className="neg"> · {s.last_error}</span>}
            </span>
          </div>
          <div className="hint">To restore: open the copy in Tallybook (Welcome → Open a book) and enter your recovery key.</div>
        </div>
      )}
    </SettingsCard>
  );
}
