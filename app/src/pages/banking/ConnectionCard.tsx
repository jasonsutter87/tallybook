// One bank connection: its status, linked accounts, and sync / reconnect / unlink actions.

import { ArrowRight, CreditCard, KeyRound, Landmark, Link2, RefreshCw, Unlink, XCircle } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router-dom";
import { call, useAction } from "../../api";
import { ConfirmModal, Spinner, useToast } from "../../components/ui";
import { date, money, plural } from "../../format";
import type { Account, BankLink, Connection, FeedAccount, SyncReport } from "../../types";
import type { LinkTarget } from "./shared";

/** SQLite `datetime('now')` is UTC without a zone marker. */
function parseTimestamp(s: string): Date {
  return new Date(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/.test(s) ? `${s.replace(" ", "T")}Z` : s);
}

function ago(s: string | null): string {
  if (!s) return "Never synced";
  const secs = Math.max(0, (Date.now() - parseTimestamp(s).getTime()) / 1000);
  if (Number.isNaN(secs)) return s;
  if (secs < 60) return "Synced just now";
  if (secs < 3600) return `Synced ${Math.floor(secs / 60)} min ago`;
  if (secs < 86400) return `Synced ${plural(Math.floor(secs / 3600), "hour")} ago`;
  return `Synced ${plural(Math.floor(secs / 86400), "day")} ago`;
}

const PROVIDER_LABEL: Record<string, string> = { plaid: "Plaid", simplefin: "SimpleFIN", file: "File" };

export default function ConnectionCard(props: { conn: Connection; accounts: Account[]; onLinkMore: (t: LinkTarget) => void; onReconnect: () => void }) {
  const { conn } = props;
  const toast = useToast();
  const sync = useAction<SyncReport[], { connection_id: number }>("bank.sync");
  const disconnect = useAction<void, { connection_id: number }>("bank.disconnect");
  const unlink = useAction<void, { link_id: number }>("bank.unlink");
  const [confirmDisconnect, setConfirmDisconnect] = useState(false);
  const [confirmUnlink, setConfirmUnlink] = useState<BankLink | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);

  const doSync = async () => {
    try {
      const [r] = await sync.mutateAsync({ connection_id: conn.id });
      if (r && !r.ok) toast.error(r.error ?? "Sync failed");
      else toast({ text: r?.imported.added ? `${plural(r.imported.added, "new transaction")} to review` : `${conn.institution} is up to date` });
    } catch (e) {
      toast.error(e);
    }
  };

  const linkMore = async () => {
    setLoadingMore(true);
    try {
      const feed = await call<FeedAccount[]>("bank.feed_accounts", { connection_id: conn.id });
      const linked = new Set(conn.links.map((l) => l.external_account_id));
      const fresh = feed.filter((a) => !linked.has(a.external_id));
      if (fresh.length === 0) toast({ text: `Every account at ${conn.institution} is already linked` });
      else props.onLinkMore({ connection_id: conn.id, institution: conn.institution, accounts: fresh });
    } catch (e) {
      toast.error(e);
    } finally {
      setLoadingMore(false);
    }
  };

  const status =
    conn.status === "ok" ? (
      <span className="pill pill-pos">Connected</span>
    ) : conn.status === "needs_relink" ? (
      <span className="pill pill-neg">Needs sign-in</span>
    ) : conn.status === "error" ? (
      <span className="pill pill-warn">Sync error</span>
    ) : (
      <span className="pill">Disconnected</span>
    );

  return (
    <section className="card">
      <div className="row-between" style={{ padding: "18px 22px 14px", flexWrap: "wrap" }}>
        <div className="row" style={{ gap: 12, minWidth: 0 }}>
          <span className="icon-bubble">
            <Landmark size={18} />
          </span>
          <div style={{ minWidth: 0 }}>
            <div className="row" style={{ gap: 8 }}>
              <span className="bold truncate" style={{ fontSize: 15.5 }}>
                {conn.institution}
              </span>
              <span className="pill pill-violet">{PROVIDER_LABEL[conn.provider] ?? conn.provider}</span>
              {status}
            </div>
            <div className="subtle small">{ago(conn.last_synced_at)}</div>
          </div>
        </div>
        <div className="row">
          {conn.status === "needs_relink" && (
            <button className="btn btn-primary btn-sm" onClick={props.onReconnect}>
              <KeyRound size={14} /> Reconnect
            </button>
          )}
          <button className="btn btn-secondary btn-sm" onClick={doSync} disabled={sync.isPending}>
            <RefreshCw size={14} className={sync.isPending ? "spin" : ""} /> {sync.isPending ? "Syncing…" : "Sync now"}
          </button>
        </div>
      </div>

      {conn.status === "error" && conn.last_error && (
        <div style={{ padding: "0 22px 14px" }}>
          <div className="banner warn small">{conn.last_error}</div>
        </div>
      )}
      {conn.status === "needs_relink" && (
        <div style={{ padding: "0 22px 14px" }}>
          <div className="banner error small">Your bank needs you to sign in again before it will share new transactions.</div>
        </div>
      )}

      {conn.links.length === 0 ? (
        <div className="subtle small" style={{ padding: "12px 22px", borderTop: "1px solid var(--line)" }}>
          No accounts linked yet — link one to start pulling in transactions.
        </div>
      ) : (
        conn.links.map((l) => {
          const ledger = props.accounts.find((a) => a.id === l.ledger_account_id);
          const differs = ledger && l.bank_balance != null && l.bank_balance !== ledger.balance;
          return (
            <div key={l.id} className="bk-link">
              <span className="icon-bubble" style={{ width: 34, height: 34 }}>
                {ledger?.subtype === "credit_card" ? <CreditCard size={16} /> : <Landmark size={16} />}
              </span>
              <div style={{ minWidth: 0 }}>
                <div className="bold truncate">
                  {l.name} {l.mask && <span className="subtle">···{l.mask}</span>}
                  <span className="bk-arrow">
                    <ArrowRight size={14} />
                  </span>
                  <Link to={`/accounts/${l.ledger_account_id}`}>{ledger?.name ?? "Account"}</Link>
                </div>
                <div className="subtle small">Since {date(l.start_date)}</div>
              </div>
              <div className="right">
                {l.bank_balance != null && (
                  <div className="small">
                    <span className="subtle">Bank </span>
                    <span className="num bold">{money(l.bank_balance)}</span>
                  </div>
                )}
                {ledger && (
                  <div className="small">
                    <span className="subtle">Books </span>
                    <span className="num bold" style={differs ? { color: "var(--warn)" } : undefined}>
                      {money(ledger.balance)}
                    </span>
                  </div>
                )}
              </div>
              <button className="btn btn-ghost btn-sm" onClick={() => setConfirmUnlink(l)}>
                <Unlink size={14} /> Unlink
              </button>
            </div>
          );
        })
      )}

      <div className="row-between" style={{ padding: "12px 22px 16px", borderTop: "1px solid var(--line)" }}>
        <button className="btn btn-ghost btn-sm" onClick={linkMore} disabled={loadingMore}>
          {loadingMore ? <Spinner /> : <Link2 size={14} />} Link another account
        </button>
        <button className="btn btn-ghost btn-sm" style={{ color: "var(--neg)" }} onClick={() => setConfirmDisconnect(true)}>
          <XCircle size={14} /> Disconnect
        </button>
      </div>

      <ConfirmModal
        open={confirmDisconnect}
        onClose={() => setConfirmDisconnect(false)}
        title={`Disconnect ${conn.institution}?`}
        body="Tallybook will stop pulling new transactions and delete the saved access from your keychain. Everything already in your books stays put."
        confirmLabel="Disconnect"
        danger
        onConfirm={async () => {
          try {
            await disconnect.mutateAsync({ connection_id: conn.id });
            toast({ text: `${conn.institution} disconnected` });
          } catch (e) {
            toast.error(e);
          }
        }}
      />
      <ConfirmModal
        open={confirmUnlink != null}
        onClose={() => setConfirmUnlink(null)}
        title="Unlink this account?"
        body={`New transactions from ${confirmUnlink?.name ?? "this account"} will stop coming in. The ledger account and everything already recorded stay put.`}
        confirmLabel="Unlink"
        danger
        onConfirm={async () => {
          if (!confirmUnlink) return;
          try {
            await unlink.mutateAsync({ link_id: confirmUnlink.id });
            toast({ text: "Account unlinked" });
          } catch (e) {
            toast.error(e);
          }
        }}
      />
    </section>
  );
}
