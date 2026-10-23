// Bank connections: connect via SimpleFIN Bridge (Plaid, via Hosted Link in the
// system browser, is hidden behind PLAID_ENABLED), link provider accounts to
// ledger accounts, sync, unlink.

import { FileSpreadsheet, Landmark, Plus, RefreshCw, ShieldCheck, Upload } from "lucide-react";
import { useState } from "react";
import { useAction, useApi } from "../api";
import { Card, EmptyState, ErrorBanner, Loading, useToast } from "../components/ui";
import ImportModal from "../flows/ImportModal";
import { plural } from "../format";
import type { Account, Connection, SyncReport } from "../types";
import "./Banking.css";
import ConnectionCard from "./banking/ConnectionCard";
import ConnectModal from "./banking/ConnectModal";
import RelinkModal from "./banking/RelinkModal";
import SimplefinRelinkModal from "./banking/SimplefinRelinkModal";
import LinkStepModal from "./banking/LinkStepModal";
import { CONNECT_START, type LinkTarget } from "./banking/shared";

// ------------------------------------------------------------- page

export default function Banking() {
  const toast = useToast();
  const connections = useApi<Connection[]>("bank.connections");
  const accounts = useApi<Account[]>("accounts.list");
  const [connectOpen, setConnectOpen] = useState<false | "choose" | "simplefin">(false);
  const [linkTarget, setLinkTarget] = useState<LinkTarget | null>(null);
  const [importOpen, setImportOpen] = useState(false);
  const [relink, setRelink] = useState<Connection | null>(null);
  const [sfRelink, setSfRelink] = useState<Connection | null>(null);
  const sync = useAction<SyncReport[], { connection_id?: number }>("bank.sync");

  const conns = (connections.data ?? []).filter((c) => c.provider !== "file" && c.status !== "disconnected");

  const syncAll = async () => {
    try {
      const reports = await sync.mutateAsync({});
      const added = reports.reduce((n, r) => n + r.imported.added, 0);
      const failed = reports.filter((r) => !r.ok);
      if (failed.length) toast.error(`${failed[0].institution}: ${failed[0].error}`);
      else toast({ text: added ? `${plural(added, "new transaction")} to review` : "You're up to date" });
    } catch (e) {
      toast.error(e);
    }
  };

  if (connections.isLoading || accounts.isLoading) return <Loading />;

  return (
    <div className="page">
      <div className="page-header">
        <div className="grow">
          <h1 className="page-title">Banking</h1>
          <div className="page-subtitle">Connect your bank and cards so transactions flow in on their own.</div>
        </div>
        <div className="row">
          {conns.length > 0 && (
            <button className="btn btn-secondary" onClick={syncAll} disabled={sync.isPending}>
              <RefreshCw size={16} className={sync.isPending ? "spin" : ""} /> {sync.isPending ? "Syncing…" : "Sync all"}
            </button>
          )}
          <button className="btn btn-primary" onClick={() => setConnectOpen(CONNECT_START)}>
            <Plus size={16} /> Connect a bank
          </button>
        </div>
      </div>

      <ErrorBanner error={connections.error ?? accounts.error} />

      <div className="bk-layout">
        <div className="stack" style={{ gap: 20 }}>
          {conns.length === 0 ? (
            <Card>
              <EmptyState
                icon={<Landmark size={24} />}
                title="No banks connected yet"
                body="Connect your bank once and new transactions show up in For Review every time you sync — no more downloading statements."
                action={
                  <div className="row" style={{ justifyContent: "center" }}>
                    <button className="btn btn-primary" onClick={() => setConnectOpen(CONNECT_START)}>
                      <Plus size={16} /> Connect a bank
                    </button>
                    <button className="btn btn-secondary" onClick={() => setImportOpen(true)}>
                      <Upload size={16} /> Import a file instead
                    </button>
                  </div>
                }
              />
            </Card>
          ) : (
            conns.map((c) => (
              <ConnectionCard
                key={c.id}
                conn={c}
                accounts={accounts.data ?? []}
                onLinkMore={(t) => setLinkTarget(t)}
                onReconnect={() => (c.provider === "plaid" ? setRelink(c) : setSfRelink(c))}
              />
            ))
          )}
        </div>

        <div className="stack" style={{ gap: 20 }}>
          <Card>
            <div className="row" style={{ gap: 12, alignItems: "flex-start" }}>
              <span className="icon-bubble violet">
                <FileSpreadsheet size={18} />
              </span>
              <div className="grow">
                <div className="bold">Prefer files?</div>
                <div className="muted small" style={{ marginTop: 4 }}>
                  Every bank lets you download a CSV, OFX or QFX statement. Import it and the transactions land in For Review — duplicates are skipped
                  automatically.
                </div>
                <button className="btn btn-secondary btn-sm" style={{ marginTop: 12 }} onClick={() => setImportOpen(true)}>
                  <Upload size={14} /> Import a statement
                </button>
              </div>
            </div>
          </Card>
          <Card>
            <div className="row" style={{ gap: 12, alignItems: "flex-start" }}>
              <span className="icon-bubble pos">
                <ShieldCheck size={18} />
              </span>
              <div className="grow">
                <div className="bold">Your credentials stay with you</div>
                <div className="muted small" style={{ marginTop: 4 }}>
                  Bank access tokens are stored in your computer's keychain, never in the book file. Tallybook talks directly to SimpleFIN —
                  there's no Tallybook server in the middle.
                </div>
              </div>
            </div>
          </Card>
        </div>
      </div>

      {connectOpen && (
        <ConnectModal
          initial={connectOpen}
          onClose={() => setConnectOpen(false)}
          onLinked={(t) => {
            setConnectOpen(false);
            setLinkTarget(t);
          }}
        />
      )}
      {relink && <RelinkModal conn={relink} onClose={() => setRelink(null)} />}
      {sfRelink && <SimplefinRelinkModal conn={sfRelink} onClose={() => setSfRelink(null)} />}
      {linkTarget && <LinkStepModal target={linkTarget} accounts={accounts.data ?? []} onClose={() => setLinkTarget(null)} />}
      <ImportModal open={importOpen} onClose={() => setImportOpen(false)} />
    </div>
  );
}
