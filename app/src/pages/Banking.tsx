// Bank connections: connect via SimpleFIN Bridge (Plaid, via Hosted Link in the
// system browser, is hidden behind PLAID_ENABLED), link provider accounts to
// ledger accounts, sync, unlink.

import {
  ArrowRight,
  CheckCircle2,
  CreditCard,
  ExternalLink,
  FileSpreadsheet,
  KeyRound,
  Landmark,
  Link2,
  Plus,
  RefreshCw,
  ShieldCheck,
  Unlink,
  Upload,
  XCircle,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Link, useNavigate } from "react-router-dom";
import { call, openExternal, queryClient, useAction, useApi } from "../api";
import { SCOPE_LABEL, useSides } from "../book";
import { Card, ConfirmModal, EmptyState, ErrorBanner, Field, Loading, Modal, MoneyInput, Spinner, useToast } from "../components/ui";
import ImportModal from "../flows/ImportModal";
import { date, iso, money, plural } from "../format";
import type { Account, AccountSubtype, BankLink, Connection, FeedAccount, FeedAccountKind, HostedLink, OpeningSuggestion, PlaidConfig, PollResult, SyncReport, Scope } from "../types";
import "./Banking.css";

// ------------------------------------------------------------- helpers

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

/** First day of the month, three months back. */
function defaultStartDate(): string {
  const d = new Date();
  return iso(new Date(d.getFullYear(), d.getMonth() - 3, 1));
}

const KIND_SUBTYPE: Record<FeedAccountKind, AccountSubtype> = {
  depository: "bank",
  credit: "credit_card",
  loan: "long_term_liability",
  investment: "other_current_asset",
  other: "other_current_asset",
};

const KIND_LABEL: Record<FeedAccountKind, string> = {
  depository: "Bank account",
  credit: "Credit card",
  loan: "Loan",
  investment: "Investment",
  other: "Account",
};

const PROVIDER_LABEL: Record<string, string> = { plaid: "Plaid", simplefin: "SimpleFIN", file: "File" };

/** New Plaid connections are hidden: bring-your-own Plaid keys cost users more
 *  than SimpleFIN and need developer approval. Existing Plaid connections keep
 *  syncing and reconnecting; the backend is untouched. */
const PLAID_ENABLED = false;
const CONNECT_START = PLAID_ENABLED ? "choose" : "simplefin";

/** A pending link-step: which connection, which provider accounts. */
interface LinkTarget {
  connection_id: number;
  institution: string;
  accounts: FeedAccount[];
}

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

// ------------------------------------------------------------- connection card

function ConnectionCard(props: { conn: Connection; accounts: Account[]; onLinkMore: (t: LinkTarget) => void; onReconnect: () => void }) {
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

// ------------------------------------------------------------- Plaid polling

/** Polls Plaid Hosted Link every 2s until the user finishes or exits. */
function usePlaidPoll(linkToken: string | null, connectionId: number | undefined, onResult: (r: PollResult) => void, onError: (e: unknown) => void) {
  const cb = useRef({ onResult, onError });
  cb.current = { onResult, onError };
  useEffect(() => {
    if (!linkToken) return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    const tick = async () => {
      try {
        const r = await call<PollResult>("bank.plaid_poll", { link_token: linkToken, connection_id: connectionId ?? null });
        if (stopped) return;
        if (r.status === "waiting") timer = setTimeout(tick, 2000);
        else cb.current.onResult(r);
      } catch (e) {
        if (!stopped) cb.current.onError(e);
      }
    };
    timer = setTimeout(tick, 2000);
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [linkToken, connectionId]);
}

function Waiting(props: { onCancel: () => void; url: string }) {
  return (
    <div className="bk-wait">
      <div className="bk-wait-ring">
        <ExternalLink size={26} />
      </div>
      <div className="bold" style={{ fontSize: 17 }}>
        Finish connecting in your browser…
      </div>
      <div className="muted small" style={{ margin: "6px auto 16px", maxWidth: 360 }}>
        Sign in to your bank on the Plaid page that just opened. This window updates on its own when you're done.
      </div>
      <div className="row" style={{ justifyContent: "center" }}>
        <button className="btn btn-ghost btn-sm" onClick={() => openExternal(props.url)}>
          Open the page again
        </button>
        <button className="btn btn-secondary btn-sm" onClick={props.onCancel}>
          Cancel
        </button>
      </div>
    </div>
  );
}

// ------------------------------------------------------------- connect modal

type ConnectStep = "choose" | "plaid_setup" | "plaid_wait" | "plaid_exited" | "simplefin";

function ConnectModal(props: { initial: "choose" | "simplefin"; onClose: () => void; onLinked: (t: LinkTarget) => void }) {
  const config = useApi<PlaidConfig>("bank.plaid_config_get", undefined, { enabled: PLAID_ENABLED });
  const [step, setStep] = useState<ConnectStep>(props.initial);
  const [error, setError] = useState<Error | null>(null);
  const [busy, setBusy] = useState(false);
  const [link, setLink] = useState<HostedLink | null>(null);

  // Plaid setup form
  const [clientId, setClientId] = useState("");
  const [secret, setSecret] = useState("");
  const [env, setEnv] = useState<"sandbox" | "production">("sandbox");
  // SimpleFIN
  const [token, setToken] = useState("");

  const startPlaid = async () => {
    setError(null);
    setBusy(true);
    try {
      const l = await call<HostedLink>("bank.plaid_start");
      setLink(l);
      setStep("plaid_wait");
      await openExternal(l.url);
    } catch (e) {
      const err = e as Error & { code?: string };
      if (err.code === "plaid_not_configured") setStep("plaid_setup");
      else setError(err);
    } finally {
      setBusy(false);
    }
  };

  usePlaidPoll(
    step === "plaid_wait" ? (link?.link_token ?? null) : null,
    undefined,
    (r) => {
      if (r.status === "linked") {
        void queryClient.invalidateQueries();
        props.onLinked({ connection_id: r.connection_id, institution: r.institution, accounts: r.accounts });
      } else if (r.status === "exited") setStep("plaid_exited");
    },
    (e) => {
      setError(e as Error);
      setStep("plaid_exited");
    },
  );

  const choosePlaid = () => {
    setError(null);
    if (config.data && !config.data.configured) setStep("plaid_setup");
    else void startPlaid();
  };

  const saveKeys = async () => {
    setError(null);
    setBusy(true);
    try {
      await call("bank.plaid_config_set", { client_id: clientId.trim(), secret: secret.trim(), environment: env });
      await queryClient.invalidateQueries({ queryKey: ["bank.plaid_config_get"] });
      setBusy(false);
      await startPlaid();
    } catch (e) {
      setError(e as Error);
      setBusy(false);
    }
  };

  const connectSimplefin = async () => {
    setError(null);
    setBusy(true);
    try {
      const r = await call<PollResult>("bank.simplefin_connect", { setup_token: token.trim() });
      if (r.status === "linked") {
        void queryClient.invalidateQueries();
        props.onLinked({ connection_id: r.connection_id, institution: r.institution, accounts: r.accounts });
      }
    } catch (e) {
      setError(e as Error);
    } finally {
      setBusy(false);
    }
  };

  const titles: Record<ConnectStep, string> = {
    choose: "Connect a bank",
    plaid_setup: "Add your Plaid keys",
    plaid_wait: "Connecting with Plaid",
    plaid_exited: "Connection cancelled",
    simplefin: "Connect with SimpleFIN",
  };

  const back = (
    <button className="btn btn-ghost" onClick={() => setStep("choose")} disabled={busy}>
      Back
    </button>
  );

  let footer: ReactNode = null;
  if (step === "plaid_setup")
    footer = (
      <>
        {back}
        <button className="btn btn-primary" onClick={saveKeys} disabled={busy || !clientId.trim() || !secret.trim()}>
          {busy ? <Spinner /> : <KeyRound size={16} />} Save keys & continue
        </button>
      </>
    );
  else if (step === "simplefin")
    footer = (
      <>
        {props.initial === "choose" ? back : <button className="btn btn-secondary" onClick={props.onClose}>Cancel</button>}
        <button className="btn btn-primary" onClick={connectSimplefin} disabled={busy || !token.trim()}>
          {busy ? <Spinner /> : <Link2 size={16} />} Connect
        </button>
      </>
    );
  else if (step === "plaid_exited")
    footer = (
      <>
        <button className="btn btn-secondary" onClick={props.onClose}>
          Close
        </button>
        <button className="btn btn-primary" onClick={startPlaid} disabled={busy}>
          {busy ? <Spinner /> : <RefreshCw size={16} />} Try again
        </button>
      </>
    );

  return (
    <Modal open onClose={props.onClose} title={titles[step]} width={560} footer={footer}>
      <div className="stack">
        <ErrorBanner error={error} />

        {step === "choose" && (
          <>
            <button className="bk-choice" onClick={choosePlaid} disabled={busy}>
              <span className="icon-bubble">{busy ? <Spinner /> : <Landmark size={18} />}</span>
              <div className="grow">
                <div className="row-between">
                  <span className="bold" style={{ fontSize: 15 }}>
                    Plaid
                  </span>
                  <span className="pill pill-brand">Any US bank</span>
                </div>
                <div className="muted small" style={{ marginTop: 4 }}>
                  Works with 12,000+ banks and cards. Uses your own free Plaid developer keys.
                </div>
                {config.data?.configured && (
                  <div className="subtle tiny" style={{ marginTop: 6 }}>
                    Using keys {config.data.client_id_hint} · {config.data.environment}{" "}
                    <button
                      type="button"
                      className="link-button"
                      onClick={(e) => {
                        e.stopPropagation();
                        setStep("plaid_setup");
                      }}
                    >
                      Change keys
                    </button>
                  </div>
                )}
              </div>
            </button>
            <button className="bk-choice" onClick={() => setStep("simplefin")} disabled={busy}>
              <span className="icon-bubble violet">
                <Link2 size={18} />
              </span>
              <div className="grow">
                <div className="row-between">
                  <span className="bold" style={{ fontSize: 15 }}>
                    SimpleFIN Bridge
                  </span>
                  <span className="pill pill-violet">~$15/yr</span>
                </div>
                <div className="muted small" style={{ marginTop: 4 }}>
                  No developer approval needed. Paste a setup token from beta-bridge.simplefin.org and you're done.
                </div>
              </div>
            </button>
          </>
        )}

        {step === "plaid_setup" && (
          <>
            <div className="banner small" style={{ alignItems: "flex-start" }}>
              <KeyRound size={16} style={{ flexShrink: 0, marginTop: 2 }} />
              <span>
                Self-hosted Tallybook uses your own free Plaid developer keys. Get them at{" "}
                <a
                  href="https://dashboard.plaid.com/developers/keys"
                  onClick={(e) => {
                    e.preventDefault();
                    void openExternal("https://dashboard.plaid.com/developers/keys");
                  }}
                >
                  dashboard.plaid.com → Developers → Keys
                </a>
                . Sandbox works with test banks (<b>user_good</b> / <b>pass_good</b>).
              </span>
            </div>
            <Field label="Client ID">
              <input className="input" value={clientId} onChange={(e) => setClientId(e.target.value)} autoFocus spellCheck={false} />
            </Field>
            <Field label="Secret" hint="Stored in your computer's keychain, never in the book file.">
              <input className="input" type="password" value={secret} onChange={(e) => setSecret(e.target.value)} spellCheck={false} />
            </Field>
            <Field label="Environment">
              <select className="select" value={env} onChange={(e) => setEnv(e.target.value as "sandbox" | "production")}>
                <option value="sandbox">Sandbox (test banks)</option>
                <option value="production">Production (real banks)</option>
              </select>
            </Field>
          </>
        )}

        {step === "plaid_wait" && link && (
          <Waiting
            url={link.url}
            onCancel={() => {
              setLink(null);
              setStep("choose");
            }}
          />
        )}

        {step === "plaid_exited" && (
          <div className="bk-wait">
            <div className="bk-wait-ring" style={{ animation: "none", background: "var(--surface-hover)", color: "var(--ink-3)" }}>
              <XCircle size={26} />
            </div>
            <div className="bold" style={{ fontSize: 17 }}>
              Cancelled
            </div>
            <div className="muted small" style={{ marginTop: 6 }}>
              The bank connection wasn't finished. You can try again whenever you're ready.
            </div>
          </div>
        )}

        {step === "simplefin" && (
          <>
            <div className="muted small">
              1. Create an account at{" "}
              <a
                href="https://beta-bridge.simplefin.org"
                onClick={(e) => {
                  e.preventDefault();
                  void openExternal("https://beta-bridge.simplefin.org");
                }}
              >
                beta-bridge.simplefin.org
              </a>{" "}
              and connect your banks there.
              <br />
              2. Click <b>New connection</b> → copy the <b>setup token</b> and paste it below. It works once.
            </div>
            <Field label="Setup token">
              <textarea
                className="textarea"
                value={token}
                onChange={(e) => setToken(e.target.value)}
                placeholder="aHR0cHM6Ly9iZXRhLWJyaWRnZS5zaW1wbGVmaW4ub3Jn…"
                spellCheck={false}
                autoFocus
                style={{ fontFamily: "var(--mono)", fontSize: 12.5 }}
              />
            </Field>
          </>
        )}
      </div>
    </Modal>
  );
}

// ------------------------------------------------------------- Plaid reconnect

function RelinkModal(props: { conn: Connection; onClose: () => void }) {
  const toast = useToast();
  const [link, setLink] = useState<HostedLink | null>(null);
  const [state, setState] = useState<"starting" | "wait" | "exited">("starting");
  const [error, setError] = useState<Error | null>(null);

  const start = async () => {
    setError(null);
    setState("starting");
    try {
      const l = await call<HostedLink>("bank.plaid_relink_start", { connection_id: props.conn.id });
      setLink(l);
      setState("wait");
      await openExternal(l.url);
    } catch (e) {
      setError(e as Error);
      setState("exited");
    }
  };

  // Start once (StrictMode runs effects twice in development; don't open two browser tabs).
  const started = useRef(false);
  useEffect(() => {
    if (started.current) return;
    started.current = true;
    void start();
  });

  usePlaidPoll(
    state === "wait" ? (link?.link_token ?? null) : null,
    props.conn.id,
    async (r) => {
      if (r.status === "exited") return setState("exited");
      try {
        await call("bank.sync", { connection_id: props.conn.id });
      } catch {
        // The sync result shows on the card; reconnecting itself worked.
      }
      await queryClient.invalidateQueries();
      toast({ text: `${props.conn.institution} reconnected` });
      props.onClose();
    },
    (e) => {
      setError(e as Error);
      setState("exited");
    },
  );

  return (
    <Modal
      open
      onClose={props.onClose}
      title={`Reconnect ${props.conn.institution}`}
      footer={
        state === "exited" ? (
          <>
            <button className="btn btn-secondary" onClick={props.onClose}>
              Close
            </button>
            <button className="btn btn-primary" onClick={start}>
              <RefreshCw size={16} /> Try again
            </button>
          </>
        ) : undefined
      }
    >
      <ErrorBanner error={error} />
      {state === "starting" && <Loading label="Opening Plaid…" />}
      {state === "wait" && link && <Waiting url={link.url} onCancel={props.onClose} />}
      {state === "exited" && !error && <div className="muted">The sign-in wasn't finished. Try again whenever you're ready.</div>}
    </Modal>
  );
}

// ------------------------------------------------------------- SimpleFIN reconnect

function SimplefinRelinkModal(props: { conn: Connection; onClose: () => void }) {
  const toast = useToast();
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      await call("bank.simplefin_reconnect", { connection_id: props.conn.id, setup_token: token.trim() });
      try {
        await call("bank.sync", { connection_id: props.conn.id });
      } catch {
        // The sync result shows on the card; reconnecting itself worked.
      }
      await queryClient.invalidateQueries();
      toast({ text: `${props.conn.institution} reconnected` });
      props.onClose();
    } catch (e) {
      setError(e as Error);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal
      open
      onClose={props.onClose}
      title={`Reconnect ${props.conn.institution}`}
      subtitle="Your linked accounts and history stay as they are."
      footer={
        <>
          <button className="btn btn-secondary" onClick={props.onClose}>
            Cancel
          </button>
          <button className="btn btn-primary" disabled={!token.trim() || busy} onClick={submit}>
            {busy ? "Reconnecting…" : "Reconnect"}
          </button>
        </>
      }
    >
      <div className="stack">
        <ErrorBanner error={error} />
        <div className="muted">
          In SimpleFIN Bridge, create a new setup token for Tallybook and paste it here. It replaces the expired access for this connection.
        </div>
        <textarea className="textarea" autoFocus value={token} onChange={(e) => setToken(e.target.value)} placeholder="Paste your SimpleFIN setup token" />
      </div>
    </Modal>
  );
}

// ------------------------------------------------------------- link step

interface LinkChoice {
  /** "new" creates a ledger account, "skip" ignores, number links to an existing account. */
  target: "new" | "skip" | number;
  name: string;
  subtype: AccountSubtype;
  scope: Scope;
  start_date: string;
}

interface OpeningDraft {
  account_id: number;
  name: string;
  date: string;
  amount: number | null;
  bank_balance: number;
  include: boolean;
}

function LinkStepModal(props: { target: LinkTarget; accounts: Account[]; onClose: () => void }) {
  const toast = useToast();
  const navigate = useNavigate();
  const { target } = props;
  const sides = useSides();
  const [choices, setChoices] = useState<LinkChoice[]>(() =>
    target.accounts.map((a) => ({
      target: "new",
      name: a.name,
      subtype: KIND_SUBTYPE[a.kind],
      scope: sides.main,
      start_date: defaultStartDate(),
    })),
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  const [openings, setOpenings] = useState<OpeningDraft[] | null>(null);
  const [added, setAdded] = useState(0);

  const existing = useMemo(
    () => props.accounts.filter((a) => !a.archived && !a.link && ["bank", "credit_card", "long_term_liability", "other_current_asset"].includes(a.subtype)),
    [props.accounts],
  );
  const chosenExisting = new Set(choices.map((c) => c.target).filter((t): t is number => typeof t === "number"));
  const active = choices.filter((c) => c.target !== "skip");
  const valid = active.length > 0 && active.every((c) => c.start_date && (c.target !== "new" || c.name.trim()));

  const set = (i: number, patch: Partial<LinkChoice>) => setChoices((cs) => cs.map((c, j) => (j === i ? { ...c, ...patch } : c)));

  const finish = (count: number) => {
    void queryClient.invalidateQueries();
    toast({ text: count ? `${target.institution} connected · ${plural(count, "transaction")} to review` : `${target.institution} connected` });
    props.onClose();
    navigate("/review");
  };

  const submit = async () => {
    setError(null);
    setBusy(true);
    try {
      const created: { id: number; name: string }[] = [];
      for (let i = 0; i < target.accounts.length; i++) {
        const fa = target.accounts[i];
        const c = choices[i];
        if (c.target === "skip") continue;
        const r = await call<{ link_id: number; account_id: number }>("bank.link", {
          connection_id: target.connection_id,
          external_account_id: fa.external_id,
          name: fa.name,
          mask: fa.mask,
          account_id: typeof c.target === "number" ? c.target : null,
          new_account: c.target === "new" ? { name: c.name.trim(), subtype: c.subtype, scope: c.scope } : null,
          start_date: c.start_date,
        });
        if (c.target === "new") created.push({ id: r.account_id, name: c.name.trim() });
      }
      const reports = await call<SyncReport[]>("bank.sync", { connection_id: target.connection_id });
      const count = reports.reduce((n, r) => n + r.imported.added - r.imported.auto_posted, 0);
      const failed = reports.find((r) => !r.ok);
      if (failed) toast.error(`${failed.institution}: ${failed.error}`);
      setAdded(count);

      // Offer opening balances for brand-new ledger accounts.
      const drafts: OpeningDraft[] = [];
      for (const a of created) {
        try {
          const s = await call<OpeningSuggestion>("bank.suggest_opening", { account_id: a.id });
          drafts.push({ account_id: a.id, name: a.name, date: s.date, amount: s.amount, bank_balance: s.bank_balance, include: true });
        } catch {
          // No balance from the bank yet — the user can set it later from the account.
        }
      }
      await queryClient.invalidateQueries();
      if (drafts.length) setOpenings(drafts);
      else finish(count);
    } catch (e) {
      setError(e as Error);
      await queryClient.invalidateQueries();
    } finally {
      setBusy(false);
    }
  };

  const saveOpenings = async () => {
    if (!openings) return;
    setError(null);
    setBusy(true);
    try {
      for (const o of openings) {
        if (!o.include || o.amount == null || !o.date) continue;
        await call("accounts.opening_balance", { id: o.account_id, date: o.date, amount: o.amount });
      }
      finish(added);
    } catch (e) {
      setError(e as Error);
    } finally {
      setBusy(false);
    }
  };

  // ---- opening balances phase
  if (openings) {
    return (
      <Modal
        open
        onClose={() => finish(added)}
        width={620}
        title="Set starting balances"
        subtitle="So your books match the bank from day one."
        footer={
          <>
            <button className="btn btn-ghost" onClick={() => finish(added)} disabled={busy}>
              Skip for now
            </button>
            <button className="btn btn-primary" onClick={saveOpenings} disabled={busy}>
              {busy ? <Spinner /> : <CheckCircle2 size={16} />} Save & review transactions
            </button>
          </>
        }
      >
        <div className="stack">
          <ErrorBanner error={error} />
          {openings.map((o, i) => (
            <div key={o.account_id} className={`bk-feed-row ${o.include ? "" : "skipped"}`}>
              <label className="switch" style={{ marginBottom: 10 }}>
                <input
                  type="checkbox"
                  checked={o.include}
                  onChange={(e) => setOpenings((os) => os!.map((x, j) => (j === i ? { ...x, include: e.target.checked } : x)))}
                />
                <span>
                  Set opening balance for <b>{o.name}</b>
                </span>
              </label>
              <div className="form-grid">
                <Field label={`Balance as of ${date(o.date)}`} hint={`Worked out from today's bank balance of ${money(o.bank_balance)}.`}>
                  <MoneyInput
                    value={o.amount}
                    allowNegative
                    onChange={(v) => setOpenings((os) => os!.map((x, j) => (j === i ? { ...x, amount: v } : x)))}
                  />
                </Field>
                <Field label="As of">
                  <input
                    className="input"
                    type="date"
                    value={o.date}
                    onChange={(e) => setOpenings((os) => os!.map((x, j) => (j === i ? { ...x, date: e.target.value } : x)))}
                  />
                </Field>
              </div>
            </div>
          ))}
        </div>
      </Modal>
    );
  }

  // ---- choose ledger accounts phase
  return (
    <Modal
      open
      onClose={props.onClose}
      width={680}
      title={`Link ${target.institution} accounts`}
      subtitle="Choose where each account's transactions should go in your books."
      footer={
        <>
          <button className="btn btn-secondary" onClick={props.onClose} disabled={busy}>
            Cancel
          </button>
          <button className="btn btn-primary" onClick={submit} disabled={!valid || busy}>
            {busy ? <Spinner /> : <Link2 size={16} />} {busy ? "Linking & syncing…" : `Link ${plural(active.length, "account")}`}
          </button>
        </>
      }
    >
      <div className="stack" style={{ gap: 12 }}>
        <ErrorBanner error={error} />
        {target.accounts.length === 0 && <div className="banner warn">The bank didn't share any accounts.</div>}
        {target.accounts.map((fa, i) => {
          const c = choices[i];
          return (
            <div key={fa.external_id} className={`bk-feed-row ${c.target === "skip" ? "skipped" : ""}`}>
              <div className="row-between" style={{ marginBottom: c.target === "skip" ? 0 : 12 }}>
                <div className="row" style={{ gap: 10, minWidth: 0 }}>
                  <span className="icon-bubble">{fa.kind === "credit" ? <CreditCard size={16} /> : <Landmark size={16} />}</span>
                  <div style={{ minWidth: 0 }}>
                    <div className="bold truncate">
                      {fa.name} {fa.mask && <span className="subtle">···{fa.mask}</span>}
                    </div>
                    <div className="subtle small">{KIND_LABEL[fa.kind]}</div>
                  </div>
                </div>
                {fa.current_balance != null && <span className="num bold">{money(fa.current_balance)}</span>}
              </div>
              <div className="form-grid">
                <Field label="Link to">
                  <select
                    className="select"
                    value={String(c.target)}
                    onChange={(e) => {
                      const v = e.target.value;
                      set(i, { target: v === "new" || v === "skip" ? v : Number(v) });
                    }}
                  >
                    <option value="new">Create a new account</option>
                    {existing.length > 0 && (
                      <optgroup label="Existing accounts">
                        {existing.map((a) => (
                          <option key={a.id} value={a.id} disabled={chosenExisting.has(a.id) && c.target !== a.id}>
                            {a.name}
                          </option>
                        ))}
                      </optgroup>
                    )}
                    <option value="skip">Skip — don't import this one</option>
                  </select>
                </Field>
                {c.target !== "skip" && (
                  <Field label="Start date">
                    <input className="input" type="date" value={c.start_date} onChange={(e) => set(i, { start_date: e.target.value })} />
                  </Field>
                )}
                {c.target === "new" && (
                  <Field label="New account name" className={sides.both ? undefined : "span-2"}>
                    <input className="input" value={c.name} onChange={(e) => set(i, { name: e.target.value })} />
                  </Field>
                )}
                {c.target === "new" && sides.both && (
                  <Field label="Whose account is it?">
                    <select className="select" value={c.scope} onChange={(e) => set(i, { scope: e.target.value as Scope })}>
                      {sides.sides.map((s) => (
                        <option key={s} value={s}>
                          {SCOPE_LABEL[s]}
                        </option>
                      ))}
                    </select>
                  </Field>
                )}
              </div>
            </div>
          );
        })}
        <div className="subtle small">
          Transactions before the start date are ignored — set an opening balance for that date so your books line up with the bank. We'll suggest one
          next.
        </div>
      </div>
    </Modal>
  );
}
