// Move from Ledger: preview and import a Ledger book into the household side.

import { Download } from "lucide-react";
import { useState } from "react";
import { call, isTauri, queryClient } from "../../api";
import { ErrorBanner, useToast } from "../../components/ui";
import { date } from "../../format";
import type { LedgerImport, LedgerPreview } from "../../types";
import { SettingsCard } from "./shared";

/** Native file picker for Ledger's database; null in a browser or when cancelled. */
async function pickLedgerFile(): Promise<string | null> {
  if (!isTauri) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const r = await open({ multiple: false, filters: [{ name: "Ledger database", extensions: ["db", "sqlite", "sqlite3"] }] });
  return typeof r === "string" ? r : null;
}

export default function LedgerCard() {
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
