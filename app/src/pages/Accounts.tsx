// Chart of accounts, grouped the way a business owner thinks about money.

import { ArrowDownLeft, Building2, ChevronRight, CreditCard, Landmark, Notebook, Plus, Receipt, Scale, Wallet } from "lucide-react";
import { useMemo, useState, type ReactNode } from "react";
import { useNavigate } from "react-router-dom";
import { useApi } from "../api";
import { SCOPE_LABEL, useSides } from "../book";
import { BigMoney, EmptyState, ErrorBanner, Loading, Money, PageHeader, Tabs } from "../components/ui";
import AddAccountModal from "../flows/AddAccountModal";
import JournalEntryModal from "../flows/JournalEntryModal";
import { FLEX_LABEL, SUBTYPE_LABEL, plural } from "../format";
import type { Account, Scope } from "../types";
import "./accounts.css";

interface Section {
  key: string;
  title: string;
  note?: string;
  icon: ReactNode;
  tone: string;
  match: (a: Account) => boolean;
}

const SECTIONS: Section[] = [
  { key: "bank", title: "Bank & cash", icon: <Landmark size={17} />, tone: "", match: (a) => a.subtype === "bank" },
  { key: "own", title: "Things you own", icon: <Building2 size={17} />, tone: "violet", match: (a) => a.kind === "asset" && a.subtype !== "bank" },
  { key: "owe", title: "Cards & loans", icon: <CreditCard size={17} />, tone: "warn", match: (a) => a.kind === "liability" },
  { key: "equity", title: "Owner's equity", icon: <Scale size={17} />, tone: "", match: (a) => a.kind === "equity" },
  { key: "income", title: "Income", note: "All time", icon: <ArrowDownLeft size={17} />, tone: "pos", match: (a) => a.kind === "income" },
  { key: "expense", title: "Expenses", note: "All time", icon: <Receipt size={17} />, tone: "neg", match: (a) => a.kind === "expense" },
];

function sortAccounts(a: Account, b: Account) {
  if (a.code && b.code) return a.code.localeCompare(b.code, undefined, { numeric: true });
  if (a.code) return -1;
  if (b.code) return 1;
  return a.name.localeCompare(b.name);
}

export default function Accounts() {
  const navigate = useNavigate();
  const { data, error, isLoading } = useApi<Account[]>("accounts.list");
  const [showArchived, setShowArchived] = useState(false);
  const [adding, setAdding] = useState(false);
  const [journal, setJournal] = useState(false);
  const { both, household, main } = useSides();
  const [side, setSide] = useState<Scope>(household ? "household" : main);
  const inSide = useMemo(() => (data ?? []).filter((a) => a.scope === side), [data, side]);

  const totals = useMemo(() => {
    const live = inSide.filter((a) => !a.archived);
    const sum = (f: (a: Account) => boolean) => live.filter(f).reduce((s, a) => s + a.balance, 0);
    const cash = sum((a) => a.subtype === "bank");
    const assets = sum((a) => a.kind === "asset");
    const owed = sum((a) => a.kind === "liability");
    return { cash, owed, net: assets - owed, archived: inSide.filter((a) => a.archived).length };
  }, [inSide]);

  if (isLoading) return <Loading />;
  if (error || !data)
    return (
      <div className="page">
        <ErrorBanner error={error} />
      </div>
    );

  const visible = inSide.filter((a) => showArchived || !a.archived);

  return (
    <div className="page">
      <PageHeader
        title="Accounts"
        subtitle="Every place money lives, comes from, or goes to."
        actions={
          <>
            <button className="btn btn-secondary" onClick={() => setJournal(true)}>
              <Notebook size={16} /> New journal entry
            </button>
            <button className="btn btn-primary" onClick={() => setAdding(true)}>
              <Plus size={16} /> Add account
            </button>
          </>
        }
      />

      <div className="stack" style={{ gap: 20 }}>
        {both && (
          <Tabs<Scope>
            value={side}
            onChange={setSide}
            tabs={[
              { value: "household", label: SCOPE_LABEL.household },
              { value: "business", label: SCOPE_LABEL.business },
            ]}
          />
        )}
        <div className="grid-3">
          <Summary icon={<Wallet size={18} />} tone="" label="Cash & bank" cents={totals.cash} foot="Across your bank accounts" />
          <Summary icon={<CreditCard size={18} />} tone="warn" label="Cards & loans" cents={totals.owed} foot={side === "household" ? "What you owe" : "What the business owes"} />
          <Summary icon={<Scale size={18} />} tone="violet" label="Net worth" cents={totals.net} foot="What you own minus what you owe" signed />
        </div>

        {totals.archived > 0 && (
          <div className="row" style={{ justifyContent: "flex-end" }}>
            <label className="switch small muted">
              <input type="checkbox" checked={showArchived} onChange={(e) => setShowArchived(e.target.checked)} />
              Show archived ({totals.archived})
            </label>
          </div>
        )}

        {SECTIONS.map((s) => {
          const accounts = visible.filter(s.match).sort(sortAccounts);
          const title = side === "household" && s.key === "expense" ? "Spending" : side === "household" && s.key === "equity" ? "Starting balances" : s.title;
          if (accounts.length === 0 && s.key !== "bank") return null;
          const total = accounts.filter((a) => !a.archived).reduce((n, a) => n + a.balance, 0);
          return (
            <section className="card" key={s.key}>
              <div className="acct-section-head">
                <div className="acct-section-title">
                  <span className={`icon-bubble ${s.tone}`}>{s.icon}</span>
                  {title}
                  <span className="subtle small" style={{ fontWeight: 600 }}>
                    {plural(accounts.length, "account")}
                  </span>
                </div>
                <div className="row">
                  {s.note && <span className="subtle small">{s.note}</span>}
                  <Money cents={total} className="bold" />
                </div>
              </div>
              {accounts.length === 0 ? (
                <EmptyState
                  icon={<Landmark size={22} />}
                  title="No bank accounts yet"
                  body="Add your checking or savings account to start tracking cash."
                  action={
                    <button className="btn btn-primary" onClick={() => setAdding(true)}>
                      <Plus size={16} /> Add a bank account
                    </button>
                  }
                />
              ) : (
                accounts.map((a) => (
                  <div
                    key={a.id}
                    className={`list-row clickable acct-row ${a.archived ? "archived" : ""}`}
                    role="link"
                    tabIndex={0}
                    onClick={() => navigate(`/accounts/${a.id}`)}
                    onKeyDown={(e) => e.key === "Enter" && navigate(`/accounts/${a.id}`)}
                  >
                    <span className="acct-code">{a.code ?? ""}</span>
                    <div className="grow">
                      <div className="row" style={{ gap: 8 }}>
                        <span className="bold truncate">{a.name}</span>
                        {a.link && <span className="pill pill-brand">Bank feed{a.link.mask ? ` ···${a.link.mask}` : ""}</span>}
                        {a.archived && <span className="pill">Archived</span>}
                      </div>
                      <div className="subtle small">{a.flex ? FLEX_LABEL[a.flex] : SUBTYPE_LABEL[a.subtype]}</div>
                    </div>
                    <Money cents={a.balance} tone="neg-only" className="acct-balance" />
                    <ChevronRight size={16} className="acct-chevron" />
                  </div>
                ))
              )}
            </section>
          );
        })}
      </div>

      <AddAccountModal open={adding} onClose={() => setAdding(false)} scope={side} />
      <JournalEntryModal open={journal} onClose={() => setJournal(false)} />
    </div>
  );
}

function Summary(props: { icon: ReactNode; tone: string; label: string; cents: number; foot: string; signed?: boolean }) {
  return (
    <div className="card card-pad">
      <div className="stat-label">
        <span className={`icon-bubble ${props.tone}`}>{props.icon}</span>
        {props.label}
      </div>
      <div className={`stat-value ${props.signed && props.cents < 0 ? "neg" : ""}`}>
        <BigMoney cents={props.cents} />
      </div>
      <div className="stat-foot">{props.foot}</div>
    </div>
  );
}
