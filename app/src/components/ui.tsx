// Shared building blocks. Pages compose these plus the CSS classes in
// styles/app.css; anything used by only one page lives with that page.

import { X } from "lucide-react";
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { Link } from "react-router-dom";
import { ArrowLeft } from "lucide-react";
import { useApi, type ApiError } from "../api";
import { useSides } from "../book";
import { KIND_ORDER, centsToInput, kindLabel, money, moneyParts, parseMoney } from "../format";
import type { Account, AccountKind, AccountSubtype, Cents, Contact, ContactKind, DocStatus, Scope } from "../types";

// ------------------------------------------------------------- layout

export function PageHeader(props: { title: ReactNode; subtitle?: ReactNode; actions?: ReactNode; back?: { to: string; label: string } }) {
  return (
    <div className="page-header">
      <div className="grow">
        {props.back && (
          <Link to={props.back.to} className="back-link">
            <ArrowLeft size={14} /> {props.back.label}
          </Link>
        )}
        <h1 className="page-title">{props.title}</h1>
        {props.subtitle && <div className="page-subtitle">{props.subtitle}</div>}
      </div>
      {props.actions && <div className="row no-print">{props.actions}</div>}
    </div>
  );
}

export function Card(props: { title?: ReactNode; action?: ReactNode; children: ReactNode; className?: string; pad?: boolean }) {
  return (
    <section className={`card ${props.className ?? ""}`}>
      {(props.title || props.action) && (
        <div className="card-header">
          <h3 className="card-title">{props.title}</h3>
          {props.action}
        </div>
      )}
      {props.pad === false ? props.children : <div className={props.title ? "card-body" : "card-pad"}>{props.children}</div>}
    </section>
  );
}

// ------------------------------------------------------------- money & status

/** Formatted cents. `tone` colors by sign: "signed" (+green/−red), "neg-only". */
export function Money(props: { cents: Cents; tone?: "signed" | "neg-only" | "none"; signed?: boolean; className?: string }) {
  const tone = props.tone ?? "none";
  const cls =
    tone === "signed" ? (props.cents > 0 ? "pos" : props.cents < 0 ? "neg" : "") : tone === "neg-only" && props.cents < 0 ? "neg" : "";
  return <span className={`num ${cls} ${props.className ?? ""}`}>{money(props.cents, { signed: props.signed })}</span>;
}

/** Big display number with de-emphasized cents. */
export function BigMoney(props: { cents: Cents; className?: string }) {
  const { whole, fraction } = moneyParts(props.cents);
  return (
    <span className={`num ${props.className ?? ""}`}>
      {whole}
      <span className="cents">{fraction}</span>
    </span>
  );
}

const STATUS: Record<DocStatus, [string, string]> = {
  draft: ["Draft", ""],
  open: ["Open", "pill-brand"],
  partial: ["Partially paid", "pill-violet"],
  paid: ["Paid", "pill-pos"],
  overdue: ["Overdue", "pill-neg"],
  void: ["Void", ""],
};

export function StatusPill({ status }: { status: DocStatus }) {
  const [label, cls] = STATUS[status];
  return <span className={`pill ${cls}`}>{label}</span>;
}

export function Avatar({ name, tone }: { name: string; tone?: number }) {
  const initials = name
    .split(/\s+/)
    .filter((w) => /[A-Za-z0-9]/.test(w[0] ?? ""))
    .slice(0, 2)
    .map((w) => w[0]!.toUpperCase())
    .join("");
  const palettes = [
    ["var(--brand-soft)", "var(--brand-soft-ink)"],
    ["var(--violet-soft)", "var(--violet)"],
    ["var(--pos-soft)", "var(--pos)"],
    ["var(--warn-soft)", "var(--warn)"],
  ];
  const [bg, fg] = palettes[(tone ?? hash(name)) % palettes.length];
  return (
    <div className="avatar" style={{ background: bg, color: fg }}>
      {initials || "•"}
    </div>
  );
}

function hash(s: string): number {
  let h = 0;
  for (const c of s) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return h;
}

// ------------------------------------------------------------- feedback

export function Spinner() {
  return <span className="spinner" aria-label="Loading" />;
}

export function Loading({ label = "Loading…" }: { label?: string }) {
  return (
    <div className="empty">
      <Spinner />
      <div className="subtle small" style={{ marginTop: 10 }}>
        {label}
      </div>
    </div>
  );
}

export function ErrorBanner({ error }: { error: ApiError | Error | null | undefined }) {
  if (!error) return null;
  return <div className="banner error">{error.message}</div>;
}

export function EmptyState(props: { icon: ReactNode; title: string; body?: ReactNode; action?: ReactNode }) {
  return (
    <div className="empty">
      <div className="empty-icon">{props.icon}</div>
      <div className="empty-title">{props.title}</div>
      {props.body && <div className="empty-body">{props.body}</div>}
      {props.action}
    </div>
  );
}

// ------------------------------------------------------------- toasts

type Toast = { id: number; text: string; error?: boolean; action?: { label: string; run: () => void } };
const ToastContext = createContext<(t: Omit<Toast, "id">) => void>(() => {});

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const push = useCallback((t: Omit<Toast, "id">) => {
    const id = Date.now() + Math.random();
    setToasts((ts) => [...ts, { ...t, id }]);
    setTimeout(() => setToasts((ts) => ts.filter((x) => x.id !== id)), t.action ? 6000 : 3200);
  }, []);
  return (
    <ToastContext.Provider value={push}>
      {children}
      <div className="toasts">
        {toasts.map((t) => (
          <div key={t.id} className={`toast ${t.error ? "error" : ""}`}>
            {t.text}
            {t.action && (
              <button
                onClick={() => {
                  t.action!.run();
                  setToasts((ts) => ts.filter((x) => x.id !== t.id));
                }}
              >
                {t.action.label}
              </button>
            )}
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
}

/** `const toast = useToast(); toast({ text: "Saved" })` or `toast.error(e)`. */
export function useToast() {
  const push = useContext(ToastContext);
  return useMemo(() => {
    const fn = (t: Omit<Toast, "id">) => push(t);
    fn.error = (e: unknown) => push({ text: e instanceof Error ? e.message : String(e), error: true });
    return fn;
  }, [push]);
}

// ------------------------------------------------------------- modal

export function Modal(props: {
  open: boolean;
  onClose: () => void;
  title: ReactNode;
  subtitle?: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
  width?: number;
}) {
  useEffect(() => {
    if (!props.open) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && props.onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [props.open, props.onClose]);
  if (!props.open) return null;
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && props.onClose()}>
      <div className="modal" style={{ maxWidth: props.width ?? 520 }} role="dialog" aria-modal="true">
        <div className="modal-header">
          <div>
            <div className="modal-title">{props.title}</div>
            {props.subtitle && <div className="muted" style={{ marginTop: 4 }}>{props.subtitle}</div>}
          </div>
          <button className="btn btn-ghost btn-icon" onClick={props.onClose} aria-label="Close">
            <X size={18} />
          </button>
        </div>
        <div className="modal-body">{props.children}</div>
        {props.footer && <div className="modal-footer">{props.footer}</div>}
      </div>
    </div>
  );
}

export function ConfirmModal(props: {
  open: boolean;
  onClose: () => void;
  onConfirm: () => void | Promise<void>;
  title: string;
  body: ReactNode;
  confirmLabel: string;
  danger?: boolean;
}) {
  const [busy, setBusy] = useState(false);
  return (
    <Modal
      open={props.open}
      onClose={props.onClose}
      title={props.title}
      footer={
        <>
          <button className="btn btn-secondary" onClick={props.onClose}>
            Cancel
          </button>
          <button
            className={`btn ${props.danger ? "btn-danger" : "btn-primary"}`}
            disabled={busy}
            onClick={async () => {
              setBusy(true);
              try {
                await props.onConfirm();
                props.onClose();
              } finally {
                setBusy(false);
              }
            }}
          >
            {props.confirmLabel}
          </button>
        </>
      }
    >
      <div className="muted">{props.body}</div>
    </Modal>
  );
}

// ------------------------------------------------------------- inputs

export function Field(props: { label: ReactNode; hint?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <label className={`field ${props.className ?? ""}`}>
      <span className="label">{props.label}</span>
      {props.children}
      {props.hint && <span className="hint">{props.hint}</span>}
    </label>
  );
}

/**
 * Money entry that keeps the user's text while typing and reports cents
 * (or null when empty/invalid). Reformats on blur.
 */
export function MoneyInput(props: {
  value: Cents | null;
  onChange: (cents: Cents | null) => void;
  placeholder?: string;
  autoFocus?: boolean;
  large?: boolean;
  className?: string;
  allowNegative?: boolean;
}) {
  const [text, setText] = useState(() => centsToInput(props.value));
  const last = useRef(props.value);
  useEffect(() => {
    if (props.value !== last.current) {
      setText(centsToInput(props.value));
      last.current = props.value;
    }
  }, [props.value]);
  const invalid = text.trim() !== "" && parseMoney(text) === null;
  return (
    <input
      className={`input money ${props.large ? "input-lg" : ""} ${props.className ?? ""}`}
      inputMode="decimal"
      placeholder={props.placeholder ?? "0.00"}
      autoFocus={props.autoFocus}
      value={text}
      style={invalid ? { borderColor: "var(--neg)" } : undefined}
      onChange={(e) => {
        setText(e.target.value);
        let c = parseMoney(e.target.value);
        if (c !== null && !props.allowNegative) c = Math.abs(c);
        last.current = c;
        props.onChange(c);
      }}
      onBlur={() => {
        const c = parseMoney(text);
        if (c !== null) setText(centsToInput(props.allowNegative ? c : Math.abs(c)));
      }}
    />
  );
}

export function Tabs<T extends string>(props: { value: T; onChange: (v: T) => void; tabs: { value: T; label: ReactNode; count?: number }[] }) {
  return (
    <div className="tabs" role="tablist">
      {props.tabs.map((t) => (
        <button key={t.value} role="tab" className={`tab ${props.value === t.value ? "active" : ""}`} onClick={() => props.onChange(t.value)}>
          {t.label}
          {t.count != null && <span className="tab-count">{t.count}</span>}
        </button>
      ))}
    </div>
  );
}

// ------------------------------------------------------------- combobox

export interface ComboOption {
  value: number;
  label: string;
  group?: string;
  detail?: ReactNode;
  keywords?: string;
}

/** Searchable select. Type to filter, arrows + Enter to pick. */
export function Combobox(props: {
  options: ComboOption[];
  value: number | null;
  onChange: (v: number | null) => void;
  placeholder?: string;
  autoFocus?: boolean;
  footer?: ReactNode;
}) {
  const id = useId();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  // True once the user has moved through the menu with the arrow keys.
  const [navigated, setNavigated] = useState(false);
  const wrap = useRef<HTMLDivElement>(null);
  const selected = props.options.find((o) => o.value === props.value);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return props.options;
    return props.options.filter((o) => `${o.label} ${o.group ?? ""} ${o.keywords ?? ""}`.toLowerCase().includes(q));
  }, [props.options, query]);

  useEffect(() => {
    const close = (e: MouseEvent) => wrap.current && !wrap.current.contains(e.target as Node) && setOpen(false);
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, []);

  useEffect(() => setActive(0), [query]);

  const pick = (o: ComboOption) => {
    props.onChange(o.value);
    setOpen(false);
    setQuery("");
    setNavigated(false);
  };

  let lastGroup: string | undefined;
  return (
    <div className="combo" ref={wrap}>
      <input
        id={id}
        className="input"
        autoFocus={props.autoFocus}
        placeholder={selected ? selected.label : (props.placeholder ?? "Choose…")}
        value={open ? query : (selected?.label ?? "")}
        onFocus={() => {
          setOpen(true);
          setNavigated(false);
        }}
        onChange={(e) => {
          setQuery(e.target.value);
          setOpen(true);
        }}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown") {
            e.preventDefault();
            setOpen(true);
            setNavigated(true);
            setActive((a) => Math.min(a + 1, filtered.length - 1));
          } else if (e.key === "ArrowUp") {
            e.preventDefault();
            setNavigated(true);
            setActive((a) => Math.max(a - 1, 0));
          } else if (e.key === "Enter" && open && props.value != null && !query.trim() && !navigated) {
            // Untouched menu over an existing choice: keep it, let Enter bubble (e.g. to submit).
            setOpen(false);
          } else if (e.key === "Enter" && open && filtered[active]) {
            e.preventDefault();
            pick(filtered[active]);
          } else if (e.key === "Escape") {
            setOpen(false);
          }
        }}
      />
      {open && (
        <div className="combo-menu" role="listbox">
          {filtered.length === 0 && <div className="combo-empty">No matches</div>}
          {filtered.map((o, i) => {
            const header = o.group && o.group !== lastGroup ? o.group : null;
            lastGroup = o.group;
            return (
              <div key={o.value}>
                {header && <div className="combo-group">{header}</div>}
                <div
                  role="option"
                  aria-selected={o.value === props.value}
                  className={`combo-option ${i === active ? "active" : ""}`}
                  onMouseEnter={() => setActive(i)}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    pick(o);
                  }}
                >
                  <span className="truncate">{o.label}</span>
                  {o.detail && <span className="subtle small">{o.detail}</span>}
                </div>
              </div>
            );
          })}
          {props.footer}
        </div>
      )}
    </div>
  );
}

/** Account picker grouped by kind. Filter with `kinds`/`subtypes`; archived accounts are hidden. */
export function AccountSelect(props: {
  value: number | null;
  onChange: (id: number | null) => void;
  kinds?: AccountKind[];
  subtypes?: AccountSubtype[];
  exclude?: number[];
  /** Groups to list first, e.g. ["expense"] for money going out. */
  prefer?: AccountKind[];
  placeholder?: string;
  autoFocus?: boolean;
  /** Only one side of the book. */
  scope?: Scope;
}) {
  const { data } = useApi<Account[]>("accounts.list");
  const { both, sides } = useSides();
  const options = useMemo<ComboOption[]>(() => {
    const accounts = (data ?? []).filter(
      (a) =>
        !a.archived &&
        (!props.scope || a.scope === props.scope) &&
        (!props.kinds || props.kinds.includes(a.kind)) &&
        (!props.subtypes || props.subtypes.includes(a.subtype)) &&
        !props.exclude?.includes(a.id),
    );
    const order = props.prefer ? [...props.prefer, ...KIND_ORDER.filter((k) => !props.prefer!.includes(k))] : KIND_ORDER;
    // In a two-sided book, household groups come first: that's where most
    // everyday money goes, and the business groups follow, labeled as such.
    const scopes: Scope[] = both ? [...sides].reverse() : [sides[0]];
    return scopes.flatMap((scope) =>
      order.flatMap((k) =>
        accounts
          .filter((a) => a.kind === k && (!both || a.scope === scope))
          .map((a) => ({ value: a.id, label: a.name, group: kindLabel(k, a.scope, both), keywords: `${a.code ?? ""} ${a.scope}` })),
      ),
    );
  }, [data, props.kinds, props.subtypes, props.exclude, props.prefer, props.scope, both, sides]);
  return <Combobox options={options} value={props.value} onChange={props.onChange} placeholder={props.placeholder ?? "Choose a category"} autoFocus={props.autoFocus} />;
}

export function ContactSelect(props: { value: number | null; onChange: (id: number | null) => void; kind: ContactKind; placeholder?: string }) {
  const { data } = useApi<Contact[]>("contacts.list");
  const options = useMemo<ComboOption[]>(
    () =>
      (data ?? [])
        .filter((c) => !c.archived && (c.kind === "both" || c.kind === props.kind))
        .map((c) => ({ value: c.id, label: c.name, keywords: `${c.company ?? ""} ${c.email ?? ""}`, detail: c.open_balance ? money(c.open_balance) : undefined })),
    [data, props.kind],
  );
  return <Combobox options={options} value={props.value} onChange={props.onChange} placeholder={props.placeholder ?? (props.kind === "vendor" ? "Choose a vendor" : "Choose a customer")} />;
}

/** Account name lookup for rendering ids. */
export function useAccountNames(): (id: number | null | undefined) => string {
  const { data } = useApi<Account[]>("accounts.list");
  return useCallback(
    (id) => {
      if (id == null) return "";
      return data?.find((a) => a.id === id)?.name ?? `#${id}`;
    },
    [data],
  );
}
