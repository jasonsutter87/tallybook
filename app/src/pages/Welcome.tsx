import { BookOpen, FolderOpen, Lock, Plus, Sparkles } from "lucide-react";
import { useState } from "react";
import { isTauri, useAction, useApi } from "../api";
import { Field, Modal, useToast } from "../components/ui";
import type { RecentBook } from "../types";

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
  const createDemo = useAction("book.create_demo");
  const open = useAction("book.open");
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");

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
    if (path) await run(create.mutateAsync({ path, company_name: name.trim() }));
  };

  const doDemo = async () => {
    if (!dir.data) return;
    const path = joinPath(dir.data, `Northwind Studio sample ${new Date().toISOString().slice(0, 19).replace(/[T:]/g, "-")}`);
    await run(createDemo.mutateAsync({ path }));
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
              Create a book for your business, or open one you already have.
            </p>
          </div>
          <button className="btn btn-primary btn-lg btn-block" onClick={() => setCreating(true)}>
            <Plus size={18} /> Create a new book
          </button>
          <button className="btn btn-secondary btn-lg btn-block" onClick={async () => { const p = await pickOpenPath(); if (p) await run(open.mutateAsync({ path: p })); }}>
            <FolderOpen size={18} /> Open a book
          </button>
          <button className="btn btn-ghost btn-block" disabled={createDemo.isPending} onClick={doDemo}>
            <Sparkles size={16} /> {createDemo.isPending ? "Building sample business…" : "Explore with sample data"}
          </button>

          {!!recent.data?.length && (
            <>
              <div className="divider" style={{ margin: "6px 0" }} />
              <div className="label">Recent</div>
              <div className="stack-sm">
                {recent.data.map((r) => (
                  <button
                    key={r.path}
                    className="list-row clickable"
                    style={{ border: "1px solid var(--line)", borderRadius: 14, background: "var(--surface)", textAlign: "left", padding: "10px 14px" }}
                    onClick={() => run(open.mutateAsync({ path: r.path }))}
                  >
                    <div className="avatar">{r.name.slice(0, 1).toUpperCase()}</div>
                    <div className="grow">
                      <div className="bold truncate">{r.name}</div>
                      <div className="subtle tiny truncate">{r.path}</div>
                    </div>
                  </button>
                ))}
              </div>
            </>
          )}
        </div>
      </div>

      <Modal
        open={creating}
        onClose={() => setCreating(false)}
        title="Name your business"
        subtitle="This appears on your invoices and reports. You can change it later."
        footer={
          <>
            <button className="btn btn-secondary" onClick={() => setCreating(false)}>
              Cancel
            </button>
            <button className="btn btn-primary" disabled={!name.trim() || create.isPending} onClick={doCreate}>
              Create book
            </button>
          </>
        }
      >
        <Field label="Business name" hint={dir.data ? `Saved to ${joinPath(dir.data, name || "…")}` : undefined}>
          <input className="input" autoFocus value={name} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === "Enter" && name.trim() && doCreate()} placeholder="e.g. Northwind Studio" />
        </Field>
      </Modal>
    </div>
  );
}
