// The first screen, before a book is open: recent books, opening one, and
// creating a business, household or combined book.

import { BookOpen, Briefcase, FolderOpen, Home, Lock, MoreHorizontal, Plus, Sparkles } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { ApiError, isTauri, useAction, useApi } from "../api";
import { Field, Modal, useToast } from "../components/ui";
import DeleteBookModal from "../flows/DeleteBookModal";
import { OFFERS, UpgradeOffer, useLicense } from "../flows/License";
import UnlockBookModal from "../flows/UnlockBookModal";
import type { RecentBook, Scope } from "../types";

/** The kinds of book on offer when creating one. `sides` goes to the backend as-is, main side first. */
const KINDS: { sides: Scope[]; label: string; body: string; icon: typeof Home }[] = [
  { sides: ["business"], label: "A business", body: "Invoices, bills, reports and tax prep.", icon: Briefcase },
  { sides: ["household"], label: "A household", body: "A monthly budget, bills coming up, and goals.", icon: Home },
  {
    sides: ["business", "household"],
    label: "Both, in one book",
    body: "For the self-employed. Each side keeps its own numbers; money you pay yourself moves between them.",
    icon: Sparkles,
  },
];

function joinPath(dir: string, name: string) {
  const safe = name.replace(/[\\/:*?"<>|]/g, "").trim() || "My Business";
  return `${dir.replace(/\/$/, "")}/${safe}.tallybook`;
}

async function pickSavePath(defaultPath: string): Promise<string | null> {
  if (!isTauri) return defaultPath;
  const { save } = await import("@tauri-apps/plugin-dialog");
  return save({ defaultPath, filters: [{ name: "Tallybook", extensions: ["tallybook"] }] });
}

async function pickOpenPath(): Promise<string | null> {
  if (!isTauri) return window.prompt("Path to a .tallybook file");
  const { open } = await import("@tauri-apps/plugin-dialog");
  const r = await open({ multiple: false, filters: [{ name: "Tallybook", extensions: ["tallybook"] }] });
  return typeof r === "string" ? r : null;
}

export default function Welcome() {
  const toast = useToast();
  const recent = useApi<RecentBook[]>("book.recent");
  const dir = useApi<string>("book.default_dir");
  const create = useAction("book.create");
  const open = useAction("book.open");
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");
  // An index into KINDS; the first, a business, is the default.
  const [kind, setKind] = useState(0);
  // Word the form for a household when that's the book's main side.
  const household = KINDS[kind].sides[0] === "household";
  const lic = useLicense();
  // A business side needs a license; a household book never does.
  const needsBusiness = KINDS[kind].sides.includes("business") && !lic.has("business");
  // Free copies start on the free choice.
  const licensed = lic.has("business");
  useEffect(() => {
    if (lic.data && !licensed) setKind(KINDS.findIndex((k) => !k.sides.includes("business")));
  }, [lic.data, licensed]);

  const [locked, setLocked] = useState<string | null>(null);
  /** Opens a book, asking for its recovery key if it's encrypted and unknown here. */
  const openBook = async (path: string) => {
    try {
      await open.mutateAsync({ path });
    } catch (e) {
      if (e instanceof ApiError && e.code === "locked") setLocked(path);
      else toast.error(e);
    }
  };
  const run = async (p: Promise<unknown>) => {
    try {
      await p;
    } catch (e) {
      toast.error(e);
    }
  };

  const doCreate = async () => {
    if (!dir.data) return;
    const path = await pickSavePath(joinPath(dir.data, name));
    if (path) await run(create.mutateAsync({ path, company_name: name.trim(), sides: KINDS[kind].sides }));
  };

  return (
    <div style={{ minHeight: "100%", display: "grid", placeItems: "center", padding: 32 }}>
      <div style={{ width: "100%", maxWidth: 940, display: "grid", gridTemplateColumns: "minmax(0, 1.05fr) minmax(0, 1fr)", gap: 28, alignItems: "stretch" }}>
        <div className="hero" style={{ padding: "40px 40px 36px", display: "flex", flexDirection: "column" }}>
          <div className="row" style={{ gap: 12 }}>
            <div className="brand-mark" style={{ background: "rgba(255,255,255,.16)", width: 40, height: 40, fontSize: 18 }}>
              T
            </div>
            <div style={{ fontWeight: 800, fontSize: 20, letterSpacing: "-.03em" }}>Tallybook</div>
          </div>
          <h1 style={{ fontSize: 40, lineHeight: 1.08, letterSpacing: "-.045em", marginTop: 48, fontWeight: 800 }}>
            Your books.
            <br />
            On your machine.
          </h1>
          <p style={{ color: "var(--hero-ink-2)", fontSize: 16, marginTop: 16, maxWidth: 380 }}>
            Real double-entry bookkeeping with bank feeds, invoices, and reports — in one file that never leaves your computer.
          </p>
          <div style={{ marginTop: "auto", paddingTop: 40 }} className="stack-sm">
            {[
              [Lock, "Bank keys live in your system keychain"],
              [BookOpen, "Every change is kept; nothing is ever silently edited"],
              [Sparkles, "Learns how you categorize, so review gets faster"],
            ].map(([Icon, text], i) => (
              <div key={i} className="row" style={{ color: "var(--hero-ink-2)", fontWeight: 600 }}>
                <Icon size={16} /> {text as string}
              </div>
            ))}
          </div>
        </div>

        <div className="card" style={{ padding: 32, display: "flex", flexDirection: "column", gap: 14 }}>
          <div>
            <h2 style={{ fontSize: 22, letterSpacing: "-.03em" }}>Get started</h2>
            <p className="muted" style={{ marginTop: 4 }}>
              Create a book for your business or your household, or open one you already have.
            </p>
          </div>
          <button className="btn btn-primary btn-lg btn-block" onClick={() => setCreating(true)}>
            <Plus size={18} /> Create a new book
          </button>
          <button className="btn btn-secondary btn-lg btn-block" onClick={async () => { const p = await pickOpenPath(); if (p) await openBook(p); }}>
            <FolderOpen size={18} /> Open a book
          </button>

          {!!recent.data?.length && (
            <>
              <div className="divider" style={{ margin: "6px 0" }} />
              <div className="label">Recent</div>
              <div className="stack-sm">
                {recent.data.slice(0, 3).map((r) => (
                  <RecentRow key={r.path} book={r} onOpen={() => openBook(r.path)} />
                ))}
              </div>
            </>
          )}
        </div>
      </div>

      <UnlockBookModal path={locked} onClose={() => setLocked(null)} />
      <Modal
        open={creating}
        onClose={() => setCreating(false)}
        title="Create a book"
        subtitle="You can add the other side later in Settings."
        footer={
          <>
            <button className="btn btn-secondary" onClick={() => setCreating(false)}>
              Cancel
            </button>
            <button className="btn btn-primary" disabled={!name.trim() || create.isPending || needsBusiness} onClick={doCreate}>
              Create book
            </button>
          </>
        }
      >
        <div className="stack-sm" role="radiogroup" aria-label="What this book keeps" style={{ marginBottom: 16 }}>
          {KINDS.map((k, i) => (
            <button
              key={k.label}
              role="radio"
              aria-checked={kind === i}
              className={`choice ${kind === i ? "active" : ""}`}
              onClick={() => setKind(i)}
            >
              <k.icon size={18} />
              <span className="grow">
                <span className="bold">{k.label}</span>
                {k.sides.includes("business") && !lic.has("business") && (
                  <span className="pill" style={{ marginLeft: 8 }}>
                    {OFFERS.business.price}
                  </span>
                )}
                <span className="muted small" style={{ display: "block" }}>
                  {k.body}
                </span>
              </span>
            </button>
          ))}
        </div>
        {needsBusiness && (
          <div style={{ marginBottom: 16 }}>
            <UpgradeOffer need="business" compact />
          </div>
        )}
        <Field
          label={household ? "Household name" : "Business name"}
          hint={dir.data ? `Saved to ${joinPath(dir.data, name || "…")}` : household ? "Shown at the top of your book." : "This appears on your invoices and reports."}
        >
          <input
            className="input"
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && name.trim() && doCreate()}
            placeholder={household ? "e.g. The Rivera household" : "e.g. Northwind Studio"}
          />
        </Field>
      </Modal>
    </div>
  );
}

/** One recent book: click to open; the ⋯ menu removes it from the list or deletes it. */
function RecentRow(props: { book: RecentBook; onOpen: () => void }) {
  const toast = useToast();
  const forget = useAction("book.forget");
  const [menu, setMenu] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const wrap = useRef<HTMLDivElement>(null);
  const r = props.book;

  useEffect(() => {
    if (!menu) return;
    const close = (e: MouseEvent) => wrap.current && !wrap.current.contains(e.target as Node) && setMenu(false);
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [menu]);

  const item = (label: string, onClick: () => void, danger = false) => (
    <button
      className="combo-option"
      style={{ width: "100%", border: 0, background: "none", textAlign: "left", color: danger ? "var(--neg)" : undefined }}
      onClick={() => {
        setMenu(false);
        onClick();
      }}
    >
      {label}
    </button>
  );

  return (
    <div ref={wrap} style={{ position: "relative" }}>
      <div className="list-row" style={{ border: "1px solid var(--line)", borderRadius: 14, background: "var(--surface)", padding: "6px 6px 6px 14px" }}>
        <button className="row grow" style={{ border: 0, background: "none", textAlign: "left", cursor: "pointer", padding: "4px 0", gap: 14, minWidth: 0 }} onClick={props.onOpen}>
          <div className="avatar">{r.name.slice(0, 1).toUpperCase()}</div>
          <div className="grow" style={{ minWidth: 0 }}>
            <div className="row" style={{ gap: 6, minWidth: 0 }}>
              <span className="bold truncate">{r.name}</span>
              {r.encrypted && <Lock size={13} className="subtle" aria-label="Encrypted" style={{ flexShrink: 0 }} />}
            </div>
            <div className="subtle tiny truncate">{r.path}</div>
          </div>
        </button>
        <button className="btn btn-ghost btn-icon" aria-label={`More for ${r.name}`} onClick={() => setMenu((m) => !m)}>
          <MoreHorizontal size={18} />
        </button>
      </div>
      {menu && (
        <div className="combo-menu" style={{ left: "auto", right: 0, width: 220, minWidth: 0 }} role="menu">
          {item("Remove from recents", () => forget.mutateAsync({ path: r.path }).catch((e) => toast.error(e)))}
          {isTauri &&
            item("Show in Finder", async () => {
              try {
                const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
                await revealItemInDir(r.path);
              } catch (e) {
                toast.error(e);
              }
            })}
          {item("Delete book…", () => setDeleting(true), true)}
        </div>
      )}
      <DeleteBookModal open={deleting} onClose={() => setDeleting(false)} path={r.path} name={r.name} />
    </div>
  );
}
