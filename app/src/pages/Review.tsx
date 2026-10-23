// For Review: every bank/card transaction lands here until the user says
// where the money went (or came from). One row open at a time.

import { CheckCheck, ChevronRight, Clock, Landmark, Link2, PartyPopper, Plus, RefreshCw, Split as SplitIcon, Tag, Trash2, Undo2, Upload, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { call, queryClient, useAction, useApi } from "../api";
import { ReviewBillNudge } from "../components/BillSuggestions";
import { PersonSelect, usePeople } from "../components/household";
import { AccountSelect, Card, ContactSelect, EmptyState, ErrorBanner, Loading, MoneyInput, Spinner, Tabs, useAccountNames, useToast } from "../components/ui";
import ImportModal from "../flows/ImportModal";
import { date, money, plural, shortDate } from "../format";
import type { Account, AccountKind, ReviewedTxn, Cents, InboxItem, Split, Suggestion, SyncReport } from "../types";
import "./Review.css";

type Tab = "review" | "reviewed";

function DateTile({ iso }: { iso: string }) {
  const [m, d] = shortDate(iso).split(" ");
  return (
    <div className="rv-date" title={date(iso)}>
      <span className="rv-date-m">{m}</span>
      <span className="rv-date-d">{d}</span>
    </div>
  );
}

function Amount({ cents }: { cents: Cents }) {
  return <div className={`rv-amount num ${cents > 0 ? "pos" : ""}`}>{cents > 0 ? money(cents, { signed: true }) : money(cents)}</div>;
}

function SuggestionChip({ s, name }: { s: Suggestion | null; name: (id: number) => string }) {
  if (!s)
    return (
      <span className="pill rv-suggest rv-suggest-none">
        <Tag size={12} /> Needs a category
      </span>
    );
  if (s.kind === "match")
    return (
      <span className="pill pill-brand rv-suggest" title={`Matches an entry already in your books: ${s.memo}`}>
        <Link2 size={12} /> Match: {s.memo || "Entry"} · {shortDate(s.date)}
      </span>
    );
  if (s.kind === "rule")
    return (
      <span className="pill pill-violet rv-suggest">
        <CheckCheck size={12} /> Rule → {name(s.account_id)}
      </span>
    );
  return (
    <span className="pill rv-suggest">
      <Clock size={12} /> Last time → {name(s.account_id)}
    </span>
  );
}

const REVIEWED_PILL: Record<string, [string, string]> = {
  posted: ["Categorized", "pill-pos"],
  matched: ["Matched", "pill-brand"],
  excluded: ["Excluded", ""],
};

/** Undo a review from a toast. Uses `call` directly so it still works after the row unmounts. */
async function undoReview(id: number) {
  await call("inbox.undo", { id });
  await queryClient.invalidateQueries();
}

export default function Review() {
  const toast = useToast();
  const name = useAccountNames();
  const [params, setParams] = useSearchParams();
  const [tab, setTab] = useState<Tab>("review");
  const [accountFilter, setAccountFilter] = useState<number | null>(null);
  const [openId, setOpenId] = useState<number | null>(null);
  const [importOpen, setImportOpen] = useState(false);
  const importParam = params.get("import");
  useEffect(() => {
    if (importParam === "1") setImportOpen(true);
  }, [importParam]);

  const inbox = useApi<InboxItem[]>("inbox.list");
  const accounts = useApi<Account[]>("accounts.list");
  const allReviewed = useApi<ReviewedTxn[]>("inbox.reviewed", { account_id: null });
  const reviewed = useApi<ReviewedTxn[]>("inbox.reviewed", { account_id: accountFilter }, { enabled: tab === "reviewed" });

  const sync = useAction<SyncReport[]>("bank.sync");
  const acceptSuggested = useAction<{ accepted: number; skipped: number }, { ids: number[] }>("inbox.accept_suggested");

  const items = inbox.data ?? [];
  const visible = accountFilter == null ? items : items.filter((i) => i.ledger_account_id === accountFilter);
  const reviewable = visible.filter((i) => !i.pending);
  const suggestedIds = reviewable.filter((i) => i.suggestion).map((i) => i.id);
  const linked = (accounts.data ?? []).some((a) => a.link);

  // Bank/card accounts that have anything in the feed (waiting or reviewed).
  const chipAccounts = useMemo(() => {
    const used = new Set<number>([...items.map((i) => i.ledger_account_id), ...(allReviewed.data ?? []).map((t) => t.ledger_account_id)]);
    return (accounts.data ?? []).filter((a) => (a.subtype === "bank" || a.subtype === "credit_card") && (used.has(a.id) || a.link));
  }, [accounts.data, items, allReviewed.data]);

  const totals = useMemo(() => {
    let inflow = 0;
    let outflow = 0;
    for (const i of reviewable) {
      if (i.amount > 0) inflow += i.amount;
      else outflow -= i.amount;
    }
    return { inflow, outflow };
  }, [reviewable]);

  const closeImport = () => {
    setImportOpen(false);
    if (params.get("import")) {
      params.delete("import");
      setParams(params, { replace: true });
    }
  };

  const doSync = async () => {
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

  const doAcceptSuggested = async () => {
    try {
      const r = await acceptSuggested.mutateAsync({ ids: suggestedIds });
      setOpenId(null);
      toast({ text: `${plural(r.accepted, "transaction")} accepted${r.skipped ? ` · ${r.skipped} skipped` : ""}` });
    } catch (e) {
      toast.error(e);
    }
  };

  /** Called by the row editor after a successful action. */
  const reviewedOne = (id: number, text: string) => {
    // Move to the next waiting row so the user can keep going.
    const idx = reviewable.findIndex((i) => i.id === id);
    const next = reviewable[idx + 1] ?? reviewable[idx - 1];
    setOpenId(next && next.id !== id ? next.id : null);
    toast({
      text,
      action: {
        label: "Undo",
        run: () => {
          undoReview(id).catch((e) => toast.error(e));
        },
      },
    });
  };

  if (inbox.isLoading || accounts.isLoading) return <Loading />;

  const waitingCount = items.filter((i) => !i.pending).length;

  return (
    <div className="page">
      <div className="page-header">
        <div className="grow">
          <h1 className="page-title">For review</h1>
          <div className="page-subtitle">
            {waitingCount > 0 ? (
              <>
                {plural(reviewable.length, "transaction")} waiting
                {totals.inflow > 0 && (
                  <>
                    {" · "}
                    <span className="pos bold">{money(totals.inflow)}</span> in
                  </>
                )}
                {totals.outflow > 0 && (
                  <>
                    {" · "}
                    <span className="bold" style={{ color: "var(--ink)" }}>
                      {money(totals.outflow)}
                    </span>{" "}
                    out
                  </>
                )}
              </>
            ) : (
              "Tell Tallybook where your money went — it learns as you go."
            )}
          </div>
        </div>
        <div className="row">
          <button className="btn btn-secondary" onClick={() => setImportOpen(true)}>
            <Upload size={16} /> Import file
          </button>
          {linked && (
            <button className="btn btn-secondary" onClick={doSync} disabled={sync.isPending}>
              <RefreshCw size={16} className={sync.isPending ? "spin" : ""} /> {sync.isPending ? "Syncing…" : "Sync banks"}
            </button>
          )}
          {tab === "review" && suggestedIds.length > 0 && (
            <button className="btn btn-primary" onClick={doAcceptSuggested} disabled={acceptSuggested.isPending}>
              {acceptSuggested.isPending ? <Spinner /> : <CheckCheck size={16} />} Accept all suggested ({suggestedIds.length})
            </button>
          )}
        </div>
      </div>

      <ErrorBanner error={inbox.error ?? accounts.error} />
      <ReviewBillNudge />

      <div className="row-between" style={{ marginBottom: 16, flexWrap: "wrap" }}>
        <Tabs<Tab>
          value={tab}
          onChange={(t) => {
            setTab(t);
            setOpenId(null);
          }}
          tabs={[
            { value: "review", label: "For review", count: waitingCount },
            { value: "reviewed", label: "Reviewed" },
          ]}
        />
        {chipAccounts.length > 1 && (
          <div className="rv-chips">
            <button className={`rv-chip ${accountFilter == null ? "active" : ""}`} onClick={() => setAccountFilter(null)}>
              All
            </button>
            {chipAccounts.map((a) => {
              const n = items.filter((i) => i.ledger_account_id === a.id && !i.pending).length;
              return (
                <button key={a.id} className={`rv-chip ${accountFilter === a.id ? "active" : ""}`} onClick={() => setAccountFilter(a.id)}>
                  {a.name}
                  {a.link?.mask && <span className="rv-chip-count">···{a.link.mask}</span>}
                  {n > 0 && <span className="rv-chip-count">· {n}</span>}
                </button>
              );
            })}
          </div>
        )}
      </div>

      {tab === "review" ? (
        visible.length === 0 ? (
          <Card>
            {items.length === 0 ? (
              <div className="rv-celebrate">
                <div className="rv-celebrate-icon">
                  <PartyPopper size={34} />
                </div>
                <div className="page-title" style={{ fontSize: 24 }}>
                  You're all caught up
                </div>
                <div className="muted" style={{ margin: "6px auto 22px", maxWidth: 420 }}>
                  Every transaction has a home. New ones from your bank or an imported statement will show up here.
                </div>
                <div className="row" style={{ justifyContent: "center" }}>
                  <button className="btn btn-secondary" onClick={() => setImportOpen(true)}>
                    <Upload size={16} /> Import a statement
                  </button>
                  <Link to="/banking" className="btn btn-primary">
                    <Landmark size={16} /> {linked ? "Manage banks" : "Connect a bank"}
                  </Link>
                </div>
              </div>
            ) : (
              <EmptyState
                icon={<CheckCheck size={22} />}
                title="Nothing waiting here"
                body={`${name(accountFilter)} is all caught up.`}
                action={
                  <button className="btn btn-secondary" onClick={() => setAccountFilter(null)}>
                    Show all accounts
                  </button>
                }
              />
            )}
          </Card>
        ) : (
          <Card pad={false}>
            {visible.map((item) => (
              <div key={item.id}>
                <div
                  className={`rv-row ${openId === item.id ? "open" : ""} ${item.pending ? "pending" : ""}`}
                  // A button to keyboard and screen-reader users too: Tab to it, Enter or Space opens it.
                  role="button"
                  tabIndex={item.pending ? -1 : 0}
                  aria-expanded={openId === item.id}
                  aria-disabled={item.pending || undefined}
                  onClick={() => !item.pending && setOpenId(openId === item.id ? null : item.id)}
                  onKeyDown={(e) => {
                    if ((e.key === "Enter" || e.key === " ") && e.target === e.currentTarget && !item.pending) {
                      e.preventDefault();
                      setOpenId(openId === item.id ? null : item.id);
                    }
                  }}
                  title={item.pending ? "Pending at the bank — you can review it once it clears" : undefined}
                >
                  <DateTile iso={item.date} />
                  <div style={{ minWidth: 0 }}>
                    <div className="bold truncate">{item.merchant || item.description}</div>
                    <div className="subtle small truncate">
                      {item.merchant && item.merchant !== item.description ? `${item.description}` : date(item.date)}
                      {accountFilter == null && ` · ${name(item.ledger_account_id)}`}
                    </div>
                  </div>
                  <div className="rv-suggest-cell">
                    {item.pending ? (
                      <span className="pill pill-warn">
                        <Clock size={12} /> Pending
                      </span>
                    ) : (
                      <SuggestionChip s={item.suggestion} name={name} />
                    )}
                  </div>
                  <Amount cents={item.amount} />
                  {item.pending ? <span /> : <ChevronRight size={16} className="rv-chevron" />}
                </div>
                {openId === item.id && <RowEditor key={item.id} item={item} onDone={reviewedOne} />}
              </div>
            ))}
          </Card>
        )
      ) : (
        <ReviewedList rows={reviewed.data} loading={reviewed.isLoading} error={reviewed.error} showAccount={accountFilter == null} />
      )}

      <ImportModal open={importOpen} onClose={closeImport} defaultAccountId={accountFilter} />
    </div>
  );
}

// ------------------------------------------------------------- reviewed tab

function ReviewedList(props: { rows: ReviewedTxn[] | undefined; loading: boolean; error: Error | null; showAccount: boolean }) {
  const name = useAccountNames();
  const toast = useToast();
  const [busy, setBusy] = useState<number | null>(null);
  if (props.loading) return <Loading />;
  if (props.error) return <ErrorBanner error={props.error} />;
  const rows = props.rows ?? [];
  if (rows.length === 0)
    return (
      <Card>
        <EmptyState icon={<CheckCheck size={22} />} title="Nothing reviewed yet" body="Transactions you categorize, match or exclude will show up here." />
      </Card>
    );
  return (
    <Card pad={false}>
      {rows.map((t) => {
        const [baseLabel, cls] = REVIEWED_PILL[t.status] ?? [t.status, ""];
        const label =
          t.categories.length === 1 ? `→ ${name(t.categories[0])}` : t.categories.length > 1 ? `Split · ${t.categories.length} categories` : baseLabel;
        return (
          <div key={t.id} className="rv-row reviewed">
            <DateTile iso={t.date} />
            <div style={{ minWidth: 0 }}>
              <div className="bold truncate">{t.merchant || t.description}</div>
              <div className="subtle small truncate">
                {date(t.date)}
                {props.showAccount && ` · ${name(t.ledger_account_id)}`}
                {t.rule_id != null && " · by a rule"}
              </div>
            </div>
            <div className="rv-suggest-cell">
              <span className={`pill ${cls}`}>{label}</span>
            </div>
            <Amount cents={t.amount} />
            <button
              className="btn btn-ghost btn-sm"
              disabled={busy === t.id}
              onClick={async () => {
                setBusy(t.id);
                try {
                  await undoReview(t.id);
                  toast({ text: "Moved back to For review" });
                } catch (e) {
                  toast.error(e);
                } finally {
                  setBusy(null);
                }
              }}
            >
              <Undo2 size={14} /> Undo
            </button>
          </div>
        );
      })}
    </Card>
  );
}

// ------------------------------------------------------------- inline editor

type Mode = "categorize" | "split" | "match";
interface SplitDraft {
  key: number;
  account_id: number | null;
  /** Positive magnitude as typed; the sign comes from the transaction. */
  amount: Cents | null;
  memo: string;
}

function RowEditor({ item, onDone }: { item: InboxItem; onDone: (id: number, text: string) => void }) {
  const toast = useToast();
  const name = useAccountNames();
  const candidates = useApi<Suggestion[]>("inbox.candidates", { id: item.id });
  const matches = (candidates.data ?? []).filter((c): c is Extract<Suggestion, { kind: "match" }> => c.kind === "match");

  const suggested = item.suggestion && item.suggestion.kind !== "match" ? item.suggestion.account_id : null;
  const [mode, setMode] = useState<Mode>(item.suggestion?.kind === "match" ? "match" : "categorize");
  const [accountId, setAccountId] = useState<number | null>(suggested);
  const [memo, setMemo] = useState("");
  const [remember, setRemember] = useState(false);
  // Who was paid / who paid you. Optional, but it's how bank payments to
  // contractors reach the 1099 summary.
  const [payeeId, setPayeeId] = useState<number | null>(null);
  const [payeeOpen, setPayeeOpen] = useState(false);
  // Who it was for, on household money.
  const [personId, setPersonId] = useState<number | null>(item.person_id);
  const accountsList = useApi<Account[]>("accounts.list");
  const { people } = usePeople();
  const scopeOf = (id: number | null) => accountsList.data?.find((a) => a.id === id)?.scope;
  // Ask "who was it for?" only on household money, and only once there are people:
  // the category picked decides, or before one is picked, the bank account's side.
  const householdMoney = people.length > 0 && (scopeOf(accountId) === "household" || (accountId == null && scopeOf(item.ledger_account_id) === "household"));
  const total = Math.abs(item.amount);
  const sign = item.amount < 0 ? -1 : 1;
  const nextKey = useRef(3);
  const [splits, setSplits] = useState<SplitDraft[]>([
    { key: 1, account_id: suggested, amount: null, memo: "" },
    { key: 2, account_id: null, amount: null, memo: "" },
  ]);
  const navigated = useRef(false);
  const excludeSelf = useMemo(() => [item.ledger_account_id], [item.ledger_account_id]);

  const accept = useAction<number, { id: number; account_id: number; memo: string | null; remember: boolean; payee_id: number | null; person_id: number | null }>("inbox.accept");
  const exclude = useAction<void, { ids: number[] }>("inbox.exclude");
  const split = useAction<number, { id: number; splits: Split[]; memo: string | null }>("inbox.split");
  const match = useAction<void, { id: number; line_id: number }>("inbox.match");
  const busy = accept.isPending || exclude.isPending || split.isPending || match.isPending;

  const label = item.merchant || item.description;
  const inflow = item.amount > 0;
  // Money in usually comes from income; money out usually goes to an expense.
  const preferKinds: AccountKind[] = inflow ? ["income", "equity", "liability"] : ["expense", "liability"];
  const isContractLabor = accountId != null && name(accountId).toLowerCase().includes("contract");
  const showPayee = payeeOpen || payeeId != null || isContractLabor;
  const effectiveMode: Mode = mode === "match" && matches.length === 0 && !candidates.isLoading ? "categorize" : mode;

  const run = async (fn: () => Promise<unknown>, text: string) => {
    try {
      await fn();
      onDone(item.id, text);
    } catch (e) {
      toast.error(e);
    }
  };

  const doAccept = () => {
    if (accountId == null || busy) return;
    void run(
      () => accept.mutateAsync({ id: item.id, account_id: accountId, memo: memo.trim() || null, remember, payee_id: payeeId, person_id: householdMoney ? personId : null }),
      remember ? `Filed under ${name(accountId)} — we'll remember ${label}` : `Filed under ${name(accountId)}`,
    );
  };

  const doExclude = () => run(() => exclude.mutateAsync({ ids: [item.id] }), "Excluded — it won't touch your books");

  const allocated = splits.reduce((n, s) => n + (s.amount ?? 0), 0);
  const remaining = total - allocated;
  const splitValid = remaining === 0 && splits.length > 0 && splits.every((s) => s.account_id != null && (s.amount ?? 0) > 0);

  const doSplit = () => {
    if (!splitValid) return;
    void run(
      () =>
        split.mutateAsync({
          id: item.id,
          memo: memo.trim() || null,
          splits: splits.map((s) => ({ account_id: s.account_id!, amount: sign * (s.amount ?? 0), memo: s.memo.trim() || null })),
        }),
      `Split across ${plural(splits.length, "category", "categories")}`,
    );
  };

  const setSplit = (key: number, patch: Partial<SplitDraft>) => setSplits((ss) => ss.map((s) => (s.key === key ? { ...s, ...patch } : s)));

  // Enter in the category picker accepts — unless the user is choosing from the menu.
  const onComboKeyCapture = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") navigated.current = true;
    if (e.key !== "Enter") return;
    const typed = (e.target as HTMLInputElement).value ?? "";
    if (typed === "" && !navigated.current && accountId != null) {
      e.preventDefault();
      e.stopPropagation();
      doAccept();
    }
    navigated.current = false;
  };
  const onComboKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Enter" && !e.defaultPrevented) doAccept();
  };

  const modes: { value: Mode; label: ReactNode }[] = [
    { value: "categorize", label: <><Tag size={13} /> Categorize</> },
    { value: "split", label: <><SplitIcon size={13} /> Split</> },
  ];
  if (matches.length > 0) modes.push({ value: "match", label: <><Link2 size={13} /> Match ({matches.length})</> });

  return (
    <div className="rv-editor" onClick={(e) => e.stopPropagation()}>
      <div className="row-between" style={{ marginBottom: 14, flexWrap: "wrap" }}>
        <Tabs<Mode> value={effectiveMode} onChange={setMode} tabs={modes} />
        <span className="subtle small">
          {inflow ? "Money in" : "Money out"} · <span className="num bold" style={{ color: "var(--ink)" }}>{money(total)}</span>
          {item.category_hint && <> · Bank calls it “{item.category_hint}”</>}
        </span>
      </div>

      {effectiveMode === "categorize" && (
        <div className="stack" style={{ gap: 14 }}>
          <div className="rv-editor-grid">
            <div className="field" onKeyDownCapture={onComboKeyCapture} onKeyDown={onComboKey}>
              <span className="label">{inflow ? "Where did this come from?" : "Where did this go?"}</span>
              <AccountSelect
                value={accountId}
                onChange={setAccountId}
                exclude={excludeSelf}
                prefer={preferKinds}
                autoFocus={suggested == null}
                aria-label={inflow ? "Where did this come from?" : "Where did this go?"}
              />
            </div>
            <label className="field">
              <span className="label">Memo (optional)</span>
              <input
                className="input"
                placeholder={item.description}
                value={memo}
                onChange={(e) => setMemo(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && doAccept()}
              />
            </label>
          </div>
          {householdMoney && (
            <div className="rv-editor-grid">
              <div className="field">
                <span className="label">
                  Who was it for? <span className="subtle" style={{ fontWeight: 500 }}>(optional)</span>
                </span>
                <PersonSelect value={personId} onChange={setPersonId} />
              </div>
              <span className="hint" style={{ paddingBottom: 10 }}>
                {remember ? `Next time, ${label} is suggested for them too.` : "Counts toward their budget for the month."}
              </span>
            </div>
          )}
          {showPayee && (
            <div className="rv-editor-grid">
              <div className="field">
                <span className="label">
                  {inflow ? "Received from" : "Paid to"} <span className="subtle" style={{ fontWeight: 500 }}>(optional)</span>
                </span>
                <div className="row" style={{ gap: 6 }}>
                  <div className="grow">
                    <ContactSelect value={payeeId} onChange={setPayeeId} kind={inflow ? "customer" : "vendor"} placeholder={inflow ? "Choose a customer" : "Choose a vendor or contractor"} />
                  </div>
                  {!isContractLabor && (
                    <button
                      className="btn btn-ghost btn-icon"
                      aria-label="Remove payee"
                      title="Remove payee"
                      onClick={() => {
                        setPayeeId(null);
                        setPayeeOpen(false);
                      }}
                    >
                      <X size={15} />
                    </button>
                  )}
                </div>
              </div>
              <span className="hint" style={{ paddingBottom: 10 }}>
                {isContractLabor
                  ? "Needed for 1099s — so Tallybook can total what you paid each contractor this year."
                  : inflow
                    ? "Links this deposit to a customer."
                    : "Tagging who you paid helps with 1099s."}
              </span>
            </div>
          )}
          <div className="row-between" style={{ flexWrap: "wrap" }}>
            <div className="row" style={{ gap: 14, flexWrap: "wrap" }}>
              <label className="switch">
                <input type="checkbox" checked={remember} onChange={(e) => setRemember(e.target.checked)} />
                <span className="small">
                  Remember for <b>{label}</b>
                </span>
              </label>
              {!showPayee && (
                <button className="btn btn-ghost btn-sm" onClick={() => setPayeeOpen(true)}>
                  <Plus size={14} /> Add payee
                </button>
              )}
            </div>
            <div className="row">
              <span className="subtle tiny" style={{ marginRight: 4 }}>
                <kbd>Enter</kbd> to accept
              </span>
              <button className="btn btn-ghost" onClick={doExclude} disabled={busy}>
                Exclude
              </button>
              <button className="btn btn-primary" onClick={doAccept} disabled={accountId == null || busy}>
                {accept.isPending ? <Spinner /> : <CheckCheck size={16} />} Accept
              </button>
            </div>
          </div>
        </div>
      )}

      {effectiveMode === "split" && (
        <div className="stack" style={{ gap: 10 }}>
          <div className="rv-split-row subtle tiny bold" style={{ textTransform: "uppercase", letterSpacing: ".06em" }}>
            <span>Category</span>
            <span style={{ textAlign: "right" }}>Amount</span>
            <span>Memo</span>
            <span />
          </div>
          {splits.map((s) => (
            <div key={s.key} className="rv-split-row">
              <AccountSelect
                value={s.account_id}
                onChange={(v) => setSplit(s.key, { account_id: v })}
                exclude={excludeSelf}
                prefer={preferKinds}
                aria-label="Split category"
              />
              <MoneyInput value={s.amount} onChange={(v) => setSplit(s.key, { amount: v })} aria-label="Split amount" />
              <input
                className="input"
                aria-label="Split memo"
                placeholder="Memo"
                value={s.memo}
                onChange={(e) => setSplit(s.key, { memo: e.target.value })}
              />
              <button
                className="btn btn-ghost btn-icon"
                aria-label="Remove line"
                disabled={splits.length <= 1}
                onClick={() => setSplits((ss) => ss.filter((x) => x.key !== s.key))}
              >
                <Trash2 size={15} />
              </button>
            </div>
          ))}
          <div className="row-between" style={{ flexWrap: "wrap", marginTop: 4 }}>
            <div className="row">
              <button
                className="btn btn-ghost btn-sm"
                onClick={() => setSplits((ss) => [...ss, { key: nextKey.current++, account_id: null, amount: remaining > 0 ? remaining : null, memo: "" }])}
              >
                <Plus size={14} /> Add line
              </button>
              {remaining > 0 && splits.some((s) => s.amount == null) && (
                <button
                  className="btn btn-ghost btn-sm"
                  onClick={() => {
                    const target = splits.find((s) => s.amount == null);
                    if (target) setSplit(target.key, { amount: remaining });
                  }}
                >
                  Fill remaining
                </button>
              )}
            </div>
            <div className="row" style={{ gap: 14 }}>
              <span className={`rv-remaining ${remaining === 0 ? "pos" : remaining < 0 ? "neg" : ""}`}>
                {remaining === 0 ? (
                  <>
                    <CheckCheck size={15} /> Adds up to {money(total)}
                  </>
                ) : remaining > 0 ? (
                  <>Remaining {money(remaining)}</>
                ) : (
                  <>Over by {money(-remaining)}</>
                )}
              </span>
              <button className="btn btn-primary" onClick={doSplit} disabled={!splitValid || busy}>
                {split.isPending ? <Spinner /> : <SplitIcon size={16} />} Save split
              </button>
            </div>
          </div>
        </div>
      )}

      {effectiveMode === "match" && (
        <div className="stack-sm">
          <div className="muted small" style={{ marginBottom: 2 }}>
            This looks like something already in your books. Matching links them so nothing is counted twice.
          </div>
          {candidates.isLoading && <Spinner />}
          {matches.map((c) => (
            <div key={c.line_id} className="rv-candidate">
              <span className="icon-bubble">
                <Link2 size={16} />
              </span>
              <div className="grow">
                <div className="bold truncate">{c.memo || "Entry"}</div>
                <div className="subtle small">
                  {date(c.date)} · {money(total)}
                </div>
              </div>
              <button
                className="btn btn-primary btn-sm"
                disabled={busy}
                onClick={() => run(() => match.mutateAsync({ id: item.id, line_id: c.line_id }), `Matched to ${c.memo || "entry"}`)}
              >
                Match
              </button>
            </div>
          ))}
          <div className="row" style={{ justifyContent: "flex-end", marginTop: 4 }}>
            <button className="btn btn-ghost" onClick={doExclude} disabled={busy}>
              Exclude
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
