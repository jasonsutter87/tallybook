// One account's register: every transaction that touched it, newest first,
// with a running balance.

import {
  Archive,
  ArchiveRestore,
  ArrowRight,
  Ban,
  Check,
  ChevronRight,
  ClipboardCheck,
  FileText,
  Lock,
  MoreHorizontal,
  Notebook,
  Pencil,
  Plus,
  Search,
  X,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { useAction, useApi } from "../api";
import {
  BigMoney,
  ConfirmModal,
  ContactSelect,
  EmptyState,
  ErrorBanner,
  Field,
  Loading,
  Modal,
  Money,
  PageHeader,
  useAccountNames,
  useToast,
} from "../components/ui";
import JournalEntryModal from "../flows/JournalEntryModal";
import LogMoneyModal, { MONEY_SUBTYPES } from "../flows/LogMoneyModal";
import { SUBTYPE_LABEL, date, money, plural, shortDate, todayISO } from "../format";
import type { Account, Cleared, Connection, Contact, Entry, RegisterRow } from "../types";
import "./accounts.css";

const PAGE = 200;

/** Column headers for the two-sided register, by account. Null = single "Amount" column. */
function columnsFor(a: Account): { minus: string; plus: string } | null {
  if (a.kind === "asset") return { minus: "Money out", plus: "Money in" };
  if (a.kind === "liability") {
    if (a.subtype === "long_term_liability") return { minus: "Repaid", plus: "Borrowed" };
    return { minus: "Payments", plus: "Charges" };
  }
  return null;
}

function balanceLabel(a: Account): string {
  if (a.subtype === "credit_card") return "You owe";
  if (a.kind === "liability") return "Balance owed";
  if (a.kind === "income") return "Earned (all time)";
  if (a.kind === "expense") return "Spent (all time)";
  if (a.kind === "equity") return "Balance";
  return "Balance in your books";
}

function ago(ts: string | null): string {
  if (!ts) return "never synced";
  const d = new Date(ts.includes("T") ? ts : ts.replace(" ", "T") + "Z");
  if (isNaN(d.getTime())) return `synced ${ts}`;
  const mins = Math.round((Date.now() - d.getTime()) / 60000);
  if (mins < 1) return "synced just now";
  if (mins < 60) return `synced ${plural(mins, "minute")} ago`;
  const hrs = Math.round(mins / 60);
  if (hrs < 24) return `synced ${plural(hrs, "hour")} ago`;
  return `synced ${d.toLocaleDateString("en-US", { month: "short", day: "numeric" })}`;
}

const CLEARED_TIP: Record<Cleared, string> = {
  uncleared: "Not yet cleared — the bank hasn't confirmed this one",
  cleared: "Cleared — matched to the bank, not yet reconciled",
  reconciled: "Reconciled — locked in a finished statement",
};

function ClearedMark({ state }: { state: Cleared }) {
  return (
    <span className={`cleared-mark ${state}`} title={CLEARED_TIP[state]} aria-label={CLEARED_TIP[state]}>
      {state === "cleared" ? <Check size={12} strokeWidth={3} /> : state === "reconciled" ? <Lock size={11} strokeWidth={2.6} /> : null}
    </span>
  );
}

export default function Register() {
  const { id: idParam } = useParams();
  const id = Number(idParam);
  const navigate = useNavigate();
  const toast = useToast();
  const accounts = useApi<Account[]>("accounts.list");
  const register = useApi<RegisterRow[]>("accounts.register", { id }, { enabled: Number.isFinite(id) });
  const account = accounts.data?.find((a) => a.id === id);
  const connections = useApi<Connection[]>("bank.connections", undefined, { enabled: !!account?.link });
  const archive = useAction<void, { id: number; archived: boolean }>("accounts.archive");

  const [query, setQuery] = useState("");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [limit, setLimit] = useState(PAGE);
  const [recording, setRecording] = useState(false);
  const [logging, setLogging] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [entryId, setEntryId] = useState<number | null>(null);
  const [menu, setMenu] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menu) return;
    const close = (e: MouseEvent) => menuRef.current && !menuRef.current.contains(e.target as Node) && setMenu(false);
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [menu]);

  useEffect(() => setLimit(PAGE), [query, from, to, id]);

  const rows = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = register.data ?? [];
    const out: RegisterRow[] = [];
    for (let i = list.length - 1; i >= 0; i--) {
      const r = list[i];
      if (q && !r.memo.toLowerCase().includes(q)) continue;
      if (from && r.date < from) continue;
      if (to && r.date > to) continue;
      out.push(r);
    }
    return out;
  }, [register.data, query, from, to]);

  const uncleared = useMemo(() => (register.data ?? []).filter((r) => r.cleared === "uncleared" && !r.voided).length, [register.data]);

  if (accounts.isLoading || register.isLoading) return <Loading />;
  if (!account)
    return (
      <div className="page">
        <PageHeader title="Account not found" back={{ to: "/accounts", label: "Accounts" }} />
        <ErrorBanner error={accounts.error ?? register.error} />
      </div>
    );

  // Cash, bank and card accounts get the plain-words form; the journal entry stays in the menu.
  const moneyAccount = MONEY_SUBTYPES.includes(account.subtype);
  const cols = columnsFor(account);
  const showCleared = account.kind === "asset" || account.kind === "liability";
  const link = account.link;
  const conn = link ? connections.data?.find((c) => c.id === link.connection_id) : undefined;
  const filtering = !!(query || from || to);

  const toggleArchive = async () => {
    setMenu(false);
    const next = !account.archived;
    try {
      await archive.mutateAsync({ id, archived: next });
      toast({
        text: next ? `${account.name} archived` : `${account.name} is active again`,
        action: { label: "Undo", run: () => archive.mutateAsync({ id, archived: !next }).catch(toast.error) },
      });
    } catch (e) {
      toast.error(e);
    }
  };

  return (
    <div className="page">
      <PageHeader
        back={{ to: "/accounts", label: "Accounts" }}
        title={account.name}
        subtitle={
          <span className="row" style={{ gap: 8 }}>
            <span className="pill">{SUBTYPE_LABEL[account.subtype]}</span>
            {account.code && <span className="subtle small">#{account.code}</span>}
            {account.archived && <span className="pill pill-warn">Archived</span>}
          </span>
        }
        actions={
          <>
            {account.reconcilable && (
              <Link to={`/accounts/${id}/reconcile`} className="btn btn-secondary">
                <ClipboardCheck size={16} /> Reconcile
              </Link>
            )}
            {!account.archived && (
              <button className="btn btn-primary" onClick={() => (moneyAccount ? setLogging(true) : setRecording(true))}>
                <Plus size={16} /> Record transaction
              </button>
            )}
            <div className="menu-wrap" ref={menuRef}>
              <button className="btn btn-ghost btn-icon" aria-label="More actions" onClick={() => setMenu((m) => !m)}>
                <MoreHorizontal size={18} />
              </button>
              {menu && (
                <div className="menu" role="menu">
                  <button
                    role="menuitem"
                    onClick={() => {
                      setMenu(false);
                      setRenaming(true);
                    }}
                  >
                    <Pencil size={15} /> Rename
                  </button>
                  {moneyAccount && !account.archived && (
                    <button
                      role="menuitem"
                      onClick={() => {
                        setMenu(false);
                        setRecording(true);
                      }}
                    >
                      <Notebook size={15} /> Journal entry…
                    </button>
                  )}
                  <button role="menuitem" className={account.archived ? "" : "danger"} onClick={toggleArchive} disabled={archive.isPending}>
                    {account.archived ? <ArchiveRestore size={15} /> : <Archive size={15} />}
                    {account.archived ? "Unarchive" : "Archive"}
                  </button>
                </div>
              )}
            </div>
          </>
        }
      />

      <div className="stack" style={{ gap: 20 }}>
        <div className="hero">
          <div className="row-between" style={{ alignItems: "flex-end", flexWrap: "wrap", position: "relative", zIndex: 1 }}>
            <div>
              <div className="hero-label">{balanceLabel(account)}</div>
              <div className="hero-amount">
                <BigMoney cents={account.balance} />
              </div>
              {link && (
                <div className="row" style={{ marginTop: 10, gap: 10, color: "var(--hero-ink-2)", fontWeight: 600 }}>
                  {link.bank_balance != null && (
                    <span>
                      Bank says <span style={{ color: "#fff" }}>{money(link.bank_balance)}</span>
                    </span>
                  )}
                  <span>
                    ···{link.mask ?? "feed"} · {conn ? ago(conn.last_synced_at) : "bank feed"}
                  </span>
                </div>
              )}
            </div>
            <div style={{ textAlign: "right", color: "var(--hero-ink-2)", fontWeight: 600 }}>
              <div>{plural(register.data?.length ?? 0, "transaction")}</div>
              {showCleared && uncleared > 0 && <div>{uncleared} not yet cleared</div>}
            </div>
          </div>
        </div>

        <section className="card">
          <div className="reg-toolbar">
            <div className="reg-search">
              <Search size={16} />
              <input className="input" placeholder="Search descriptions" value={query} onChange={(e) => setQuery(e.target.value)} />
            </div>
            <input className="input reg-date" type="date" value={from} onChange={(e) => setFrom(e.target.value)} aria-label="From date" title="From date" />
            <span className="subtle">to</span>
            <input className="input reg-date" type="date" value={to} onChange={(e) => setTo(e.target.value)} aria-label="To date" title="To date" />
            {filtering && (
              <button
                className="btn btn-ghost btn-sm"
                onClick={() => {
                  setQuery("");
                  setFrom("");
                  setTo("");
                }}
              >
                <X size={14} /> Clear
              </button>
            )}
          </div>

          <ErrorBanner error={register.error} />

          {rows.length === 0 ? (
            filtering ? (
              <EmptyState icon={<Search size={22} />} title="Nothing matches" body="Try a different search or date range." />
            ) : (
              <EmptyState
                icon={<FileText size={22} />}
                title="No transactions yet"
                body="Transactions from bank feeds, invoices, bills and manual entries will show up here."
                action={
                  !account.archived && (
                    <button className="btn btn-primary" onClick={() => setRecording(true)}>
                      <Plus size={16} /> Record transaction
                    </button>
                  )
                }
              />
            )
          ) : (
            <>
              <div style={{ overflowX: "auto" }}>
                <table className="table reg-table">
                  <thead>
                    <tr>
                      <th style={{ width: 110 }}>Date</th>
                      <th>Description</th>
                      {showCleared && <th style={{ width: 44 }} title="Cleared status" />}
                      {cols ? (
                        <>
                          <th className="right">{cols.minus}</th>
                          <th className="right">{cols.plus}</th>
                        </>
                      ) : (
                        <th className="right">Amount</th>
                      )}
                      <th className="right">{cols ? "Balance" : "Running total"}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {rows.slice(0, limit).map((r) => (
                      <tr key={r.line_id} className={`clickable ${r.voided ? "voided" : ""}`} onClick={() => setEntryId(r.entry_id)}>
                        <td className="nowrap muted">{date(r.date)}</td>
                        <td>
                          <div className="row" style={{ gap: 8 }}>
                            <button type="button" className="row-button reg-memo truncate" onClick={() => setEntryId(r.entry_id)}>
                              {r.memo || <span className="subtle">No description</span>}
                            </button>
                            {r.voided && <span className="pill">Void</span>}
                          </div>
                        </td>
                        {showCleared && (
                          <td>
                            <ClearedMark state={r.cleared} />
                          </td>
                        )}
                        {cols ? (
                          <>
                            <td className="right num reg-amt">{r.amount < 0 ? money(-r.amount) : ""}</td>
                            <td className={`right num reg-amt ${account.kind === "asset" && r.amount > 0 && !r.voided ? "pos" : ""}`}>
                              {r.amount > 0 ? money(r.amount) : ""}
                            </td>
                          </>
                        ) : (
                          <td className="right num reg-amt">{money(r.amount)}</td>
                        )}
                        <td className="right">
                          <Money cents={r.balance} tone="neg-only" className="bold" />
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              {rows.length > limit && (
                <div className="reg-more">
                  <button className="btn btn-secondary btn-sm" onClick={() => setLimit((l) => l + PAGE)}>
                    Show {Math.min(PAGE, rows.length - limit)} more · {rows.length - limit} older
                  </button>
                </div>
              )}
            </>
          )}
        </section>
      </div>

      <JournalEntryModal open={recording} onClose={() => setRecording(false)} defaultAccountId={id} />
      <LogMoneyModal open={logging} onClose={() => setLogging(false)} scope={account.scope} accountId={account.id} />
      <RenameModal open={renaming} onClose={() => setRenaming(false)} account={account} />
      <EntryModal entryId={entryId} onClose={() => setEntryId(null)} onOpenEntry={setEntryId} onNavigate={navigate} />
    </div>
  );
}

// ------------------------------------------------------------- rename

function RenameModal(props: { open: boolean; onClose: () => void; account: Account }) {
  const toast = useToast();
  const rename = useAction<void, { id: number; name: string }>("accounts.rename");
  const [name, setName] = useState(props.account.name);
  useEffect(() => {
    if (props.open) {
      setName(props.account.name);
      rename.reset();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.open]);

  const save = async () => {
    if (!name.trim() || name.trim() === props.account.name) return props.onClose();
    try {
      await rename.mutateAsync({ id: props.account.id, name: name.trim() });
      toast({ text: "Renamed" });
      props.onClose();
    } catch {
      /* inline */
    }
  };

  return (
    <Modal
      open={props.open}
      onClose={props.onClose}
      title="Rename account"
      footer={
        <>
          <button className="btn btn-secondary" onClick={props.onClose}>
            Cancel
          </button>
          <button className="btn btn-primary" disabled={!name.trim() || rename.isPending} onClick={save}>
            Save
          </button>
        </>
      }
    >
      <form
        className="stack"
        onSubmit={(e) => {
          e.preventDefault();
          save();
        }}
      >
        <ErrorBanner error={rename.error} />
        <Field label="Name" hint="History stays attached — only the label changes.">
          <input className="input" autoFocus value={name} onChange={(e) => setName(e.target.value)} />
        </Field>
      </form>
    </Modal>
  );
}

// ------------------------------------------------------------- entry detail

const SOURCE_LABEL: Record<string, string> = {
  manual: "Entered by hand",
  bank_feed: "Bank feed",
  invoice: "Invoice",
  bill: "Bill",
  payment: "Payment",
  void: "Void",
  opening_balance: "Opening balance",
};

function EntryModal(props: { entryId: number | null; onClose: () => void; onOpenEntry: (id: number) => void; onNavigate: (to: string) => void }) {
  const toast = useToast();
  const names = useAccountNames();
  const entry = useApi<Entry>("entries.get", { id: props.entryId }, { enabled: props.entryId != null });
  const voidEntry = useAction<number, { id: number; date: string }>("entries.void");
  const accounts = useApi<Account[]>("accounts.list");
  const contacts = useApi<Contact[]>("contacts.list");
  const setPayee = useAction<null, { entry_id: number; contact_id: number | null }>("tax.set_payee");
  const [editingPayee, setEditingPayee] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [voidDate, setVoidDate] = useState(todayISO());

  useEffect(() => {
    setConfirming(false);
    setVoidDate(todayISO());
    setEditingPayee(false);
  }, [props.entryId]);

  const e = entry.data;
  const fromLines = e?.lines.filter((l) => l.amount < 0) ?? [];
  const toLines = e?.lines.filter((l) => l.amount > 0) ?? [];
  const total = toLines.reduce((s, l) => s + l.amount, 0);
  const docSource = e && (e.source === "invoice" || e.source === "bill" || e.source === "payment");
  const canVoid = e && !e.voided_by_id && !e.reverses_id && !docSource;
  const locked = e?.lines.some((l) => l.cleared === "reconciled");
  // Money into a bank/card account came from a customer; money out went to a vendor.
  const cashLine = e?.lines.find((l) => {
    const st = accounts.data?.find((a) => a.id === l.account_id)?.subtype;
    return st === "bank" || st === "credit_card";
  });
  const payeeKind = cashLine && cashLine.amount > 0 ? "customer" : "vendor";
  const payeeName = e?.payee_id != null ? (contacts.data?.find((c) => c.id === e.payee_id)?.name ?? `Contact #${e.payee_id}`) : null;
  const changePayee = async (contactId: number | null) => {
    if (!e) return;
    try {
      await setPayee.mutateAsync({ entry_id: e.id, contact_id: contactId });
      setEditingPayee(false);
      toast({ text: contactId == null ? "Payee removed" : `Tagged ${contacts.data?.find((c) => c.id === contactId)?.name ?? "payee"}` });
    } catch (err) {
      toast.error(err);
    }
  };

  return (
    <>
      <Modal
        open={props.entryId != null && !confirming}
        onClose={props.onClose}
        title={e ? e.memo || "Transaction" : "Transaction"}
        subtitle={e ? `${date(e.date)} · ${SOURCE_LABEL[e.source] ?? e.source}` : undefined}
        width={600}
        footer={
          <>
            {canVoid && (
              <button className="btn btn-danger" style={{ marginRight: "auto" }} onClick={() => setConfirming(true)} disabled={locked}
                title={locked ? "Part of a finished reconciliation — undo that first" : undefined}>
                <Ban size={15} /> Void this transaction
              </button>
            )}
            <button className="btn btn-secondary" onClick={props.onClose}>
              Close
            </button>
          </>
        }
      >
        {entry.isLoading || !e ? (
          entry.error ? <ErrorBanner error={entry.error} /> : <Loading />
        ) : (
          <div className="stack">
            {e.voided_by_id && (
              <div className="banner warn">
                <Ban size={16} />
                <span className="grow">This transaction was voided. It stays in your history, cancelled out by a matching reversal.</span>
                <button className="btn btn-ghost btn-sm" onClick={() => props.onOpenEntry(e.voided_by_id!)}>
                  See the void <ChevronRight size={14} />
                </button>
              </div>
            )}
            {e.reverses_id && (
              <div className="banner">
                <span className="grow">This entry cancels out an earlier transaction.</span>
                <button className="btn btn-ghost btn-sm" onClick={() => props.onOpenEntry(e.reverses_id!)}>
                  See the original <ChevronRight size={14} />
                </button>
              </div>
            )}

            <div className="row-between">
              <span className="muted bold">Amount</span>
              <span className="num" style={{ fontSize: 26, fontWeight: 800, letterSpacing: "-.03em" }}>
                {money(total)}
              </span>
            </div>

            {fromLines.length === 1 && toLines.length === 1 ? (
              <div className="flow-line">
                <div className="flow-acct">
                  <span className="label">From</span>
                  <span className="bold">{names(fromLines[0].account_id)}</span>
                </div>
                <ArrowRight size={18} className="subtle" />
                <div className="flow-acct" style={{ textAlign: "right" }}>
                  <span className="label">To</span>
                  <span className="bold">{names(toLines[0].account_id)}</span>
                </div>
              </div>
            ) : (
              <div className="grid-2">
                <FlowList title="From" lines={fromLines.map((l) => ({ name: names(l.account_id), amount: -l.amount, memo: l.memo }))} />
                <FlowList title="To" lines={toLines.map((l) => ({ name: names(l.account_id), amount: l.amount, memo: l.memo }))} />
              </div>
            )}

            <div className="row-between" style={{ gap: 12, minHeight: 40 }}>
              <span className="muted bold nowrap">{payeeKind === "customer" ? "Received from" : "Paid to"}</span>
              {editingPayee ? (
                <div className="row" style={{ gap: 6, flex: 1, maxWidth: 340 }}>
                  <div className="grow">
                    <ContactSelect value={e.payee_id} onChange={(id) => id != null && changePayee(id)} kind={payeeKind} />
                  </div>
                  <button className="btn btn-ghost btn-sm" onClick={() => setEditingPayee(false)}>
                    Cancel
                  </button>
                </div>
              ) : (
                <div className="row" style={{ gap: 6 }}>
                  {payeeName ? <span className="bold">{payeeName}</span> : <span className="subtle">No payee</span>}
                  {!docSource && (
                    <>
                      <button className="btn btn-ghost btn-sm" onClick={() => setEditingPayee(true)} disabled={setPayee.isPending}>
                        {payeeName ? "Change" : "Add payee"}
                      </button>
                      {payeeName && (
                        <button className="btn btn-ghost btn-sm" onClick={() => changePayee(null)} disabled={setPayee.isPending}>
                          Remove
                        </button>
                      )}
                    </>
                  )}
                </div>
              )}
            </div>
            {cashLine && payeeKind === "vendor" && !payeeName && !editingPayee && (
              <div className="subtle small" style={{ marginTop: -10 }}>
                Paid a contractor? Tagging them here counts this payment toward their 1099.
              </div>
            )}

            {docSource && (
              <div className="banner">
                <FileText size={16} />
                <span className="grow">
                  This came from {e.source === "invoice" ? "an invoice" : e.source === "bill" ? "a bill" : "a payment"}. To cancel it, void it from that
                  document so everything stays in sync.
                </span>
                {e.source !== "bill" && (
                  <button className="btn btn-ghost btn-sm" onClick={() => props.onNavigate("/invoices")}>
                    Invoices
                  </button>
                )}
                {e.source !== "invoice" && (
                  <button className="btn btn-ghost btn-sm" onClick={() => props.onNavigate("/bills")}>
                    Bills
                  </button>
                )}
              </div>
            )}
            {canVoid && locked && <div className="banner warn">This is part of a finished reconciliation. Undo that reconciliation before voiding.</div>}

            <details className="details-disclosure">
              <summary>
                <ChevronRight size={14} className="chev" /> Accounting details
              </summary>
              <div className="stack-sm" style={{ marginTop: 12 }}>
                <table className="table">
                  <thead>
                    <tr>
                      <th>Account</th>
                      <th className="right">Debit</th>
                      <th className="right">Credit</th>
                      <th style={{ width: 40 }} />
                    </tr>
                  </thead>
                  <tbody>
                    {e.lines.map((l) => (
                      <tr key={l.id}>
                        <td>
                          <div className="bold">{names(l.account_id)}</div>
                          {l.memo && <div className="subtle small">{l.memo}</div>}
                        </td>
                        <td className="right num">{l.amount > 0 ? money(l.amount) : ""}</td>
                        <td className="right num">{l.amount < 0 ? money(-l.amount) : ""}</td>
                        <td>
                          <ClearedMark state={l.cleared} />
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                <div className="subtle small">Entry #{e.id} · source: {e.source}</div>
              </div>
            </details>
          </div>
        )}
      </Modal>

      <ConfirmModal
        open={confirming}
        onClose={() => setConfirming(false)}
        title="Void this transaction?"
        confirmLabel="Void transaction"
        danger
        onConfirm={async () => {
          if (!e) return;
          try {
            await voidEntry.mutateAsync({ id: e.id, date: voidDate });
            toast({ text: "Transaction voided" });
            props.onClose();
          } catch (err) {
            toast.error(err);
          }
        }}
        body={
          <div className="stack">
            <span>
              Voiding doesn't delete anything. Tallybook adds a matching reversal so the two cancel out, and both stay in your history for your
              records.
            </span>
            <Field label="Date of the void" hint={e ? `Can't be before ${shortDate(e.date)}` : undefined}>
              <input className="input" type="date" value={voidDate} min={e?.date} onChange={(ev) => setVoidDate(ev.target.value)} />
            </Field>
          </div>
        }
      />
    </>
  );
}

function FlowList(props: { title: string; lines: { name: string; amount: number; memo: string | null }[] }) {
  return (
    <div className="flow-line" style={{ flexDirection: "column", alignItems: "stretch", gap: 8 }}>
      <span className="label">{props.title}</span>
      {props.lines.map((l, i) => (
        <div key={i} className="row-between">
          <span className="bold truncate">{l.name}</span>
          <span className="num">{money(l.amount)}</span>
        </div>
      ))}
    </div>
  );
}
