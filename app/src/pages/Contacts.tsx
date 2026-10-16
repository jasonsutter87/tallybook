import { Archive, ArchiveRestore, FileText, Mail, MapPin, Pencil, Phone, Plus, Receipt, Search, StickyNote, Users, X } from "lucide-react";
import { useEffect, useMemo, useState, type ReactNode } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { useAction, useApi } from "../api";
import { Avatar, EmptyState, ErrorBanner, Loading, Money, PageHeader, StatusPill, Tabs, useToast } from "../components/ui";
import ContactModal from "../flows/ContactModal";
import { date, money, plural } from "../format";
import type { BillSummary, Contact, ContactKind, InvoiceSummary } from "../types";
import "./bills.css";

type Tab = "customer" | "vendor" | "all";

const KIND_LABEL: Record<ContactKind, string> = { customer: "Customer", vendor: "Vendor", both: "Customer & vendor" };
const KIND_PILL: Record<ContactKind, string> = { customer: "pill-brand", vendor: "pill-violet", both: "pill-warn" };

const isCustomer = (c: Contact) => c.kind === "customer" || c.kind === "both";
const isVendor = (c: Contact) => c.kind === "vendor" || c.kind === "both";

export default function Contacts() {
  const [params, setParams] = useSearchParams();
  const { data, error, isLoading } = useApi<Contact[]>("contacts.list");
  const [tab, setTab] = useState<Tab>("all");
  const [query, setQuery] = useState("");
  const [showArchived, setShowArchived] = useState(false);
  const [editing, setEditing] = useState<{ contact?: Contact; kind?: ContactKind } | null>(null);

  const contacts = data ?? [];
  const selectedId = Number(params.get("contact")) || null;
  const selected = contacts.find((c) => c.id === selectedId) ?? null;
  const select = (id: number | null) => {
    const next = new URLSearchParams(params);
    if (id == null) next.delete("contact");
    else next.set("contact", String(id));
    setParams(next, { replace: true });
  };

  const counts = useMemo(() => {
    const live = contacts.filter((c) => showArchived || !c.archived);
    return { customer: live.filter(isCustomer).length, vendor: live.filter(isVendor).length, all: live.length };
  }, [contacts, showArchived]);
  const archivedCount = contacts.filter((c) => c.archived).length;

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return contacts
      .filter((c) => showArchived || !c.archived)
      .filter((c) => (tab === "customer" ? isCustomer(c) : tab === "vendor" ? isVendor(c) : true))
      .filter((c) => !q || `${c.name} ${c.company ?? ""} ${c.email ?? ""} ${c.phone ?? ""}`.toLowerCase().includes(q))
      .sort((a, b) => Number(a.archived) - Number(b.archived) || a.name.localeCompare(b.name));
  }, [contacts, tab, query, showArchived]);

  if (isLoading) return <Loading />;

  return (
    <div className="page">
      <PageHeader
        title="Contacts"
        subtitle="The customers you bill and the vendors you pay."
        actions={
          <button className="btn btn-primary" onClick={() => setEditing({ kind: tab === "vendor" ? "vendor" : "customer" })}>
            <Plus size={16} /> Add contact
          </button>
        }
      />

      <div className="stack" style={{ gap: 18 }}>
        <ErrorBanner error={error} />

        <div className="row-between" style={{ flexWrap: "wrap", gap: 12 }}>
          <Tabs
            value={tab}
            onChange={setTab}
            tabs={[
              { value: "all", label: "All", count: counts.all },
              { value: "customer", label: "Customers", count: counts.customer },
              { value: "vendor", label: "Vendors", count: counts.vendor },
            ]}
          />
          <div className="row" style={{ gap: 16 }}>
            {archivedCount > 0 && (
              <label className="switch small">
                <input type="checkbox" checked={showArchived} onChange={(e) => setShowArchived(e.target.checked)} />
                Show archived ({archivedCount})
              </label>
            )}
            <div className="bk-search">
              <Search size={15} />
              <input className="input" placeholder="Search name, email, phone" value={query} onChange={(e) => setQuery(e.target.value)} />
            </div>
          </div>
        </div>

        {shown.length === 0 ? (
          <div className="card">
            {contacts.length === 0 ? (
              <EmptyState
                icon={<Users size={22} />}
                title="No contacts yet"
                body="Add the people and businesses you work with — customers you invoice and vendors whose bills you pay."
                action={
                  <button className="btn btn-primary" onClick={() => setEditing({ kind: "customer" })}>
                    <Plus size={16} /> Add your first contact
                  </button>
                }
              />
            ) : (
              <EmptyState icon={<Search size={22} />} title="No matches" body={query ? "Try a different name or email." : "Nobody in this group yet."} />
            )}
          </div>
        ) : (
          <div className="grid-3">
            {shown.map((c) => (
              <ContactCard key={c.id} c={c} onClick={() => select(c.id)} />
            ))}
          </div>
        )}
      </div>

      {selected && (
        <Profile
          contact={selected}
          onClose={() => select(null)}
          onEdit={() => setEditing({ contact: selected })}
        />
      )}

      <ContactModal
        open={editing != null}
        onClose={() => setEditing(null)}
        contact={editing?.contact}
        defaultKind={editing?.kind}
        onSaved={(id) => !editing?.contact && select(id)}
      />
    </div>
  );
}

function balanceLabel(c: Contact): { text: string; color: string } | null {
  if (!c.open_balance) return null;
  if (c.kind === "vendor") return { text: `You owe ${money(c.open_balance)}`, color: "var(--warn)" };
  if (c.kind === "customer") return { text: `Owes you ${money(c.open_balance)}`, color: "var(--pos)" };
  return { text: `Open balance ${money(c.open_balance)}`, color: "var(--ink)" };
}

function ContactCard({ c, onClick }: { c: Contact; onClick: () => void }) {
  const bal = balanceLabel(c);
  return (
    <button className={`card bk-contact ${c.archived ? "archived" : ""}`} onClick={onClick}>
      <div className="row" style={{ gap: 12, width: "100%" }}>
        <Avatar name={c.name} />
        <div className="grow" style={{ minWidth: 0 }}>
          <div className="bold truncate">{c.name}</div>
          <div className="small subtle truncate">{c.company || KIND_LABEL[c.kind]}</div>
        </div>
        {c.archived ? <span className="pill">Archived</span> : <span className={`pill ${KIND_PILL[c.kind]}`}>{c.kind === "both" ? "Both" : KIND_LABEL[c.kind]}</span>}
      </div>
      <div className="stack-sm small muted" style={{ gap: 4, minHeight: 38 }}>
        {c.email && (
          <span className="row truncate" style={{ gap: 6 }}>
            <Mail size={13} /> {c.email}
          </span>
        )}
        {c.phone && (
          <span className="row" style={{ gap: 6 }}>
            <Phone size={13} /> {c.phone}
          </span>
        )}
        {!c.email && !c.phone && <span className="subtle">No contact info</span>}
      </div>
      <div className="foot row-between small" style={{ width: "100%" }}>
        {bal ? (
          <span className="bold" style={{ color: bal.color }}>
            {bal.text}
          </span>
        ) : (
          <span className="subtle">All settled</span>
        )}
      </div>
    </button>
  );
}

// ------------------------------------------------------------- profile panel

function Profile(props: { contact: Contact; onClose: () => void; onEdit: () => void }) {
  const c = props.contact;
  const toast = useToast();
  const archive = useAction<null, { id: number; archived: boolean }>("contacts.archive");
  const invoices = useApi<InvoiceSummary[]>("invoices.list", undefined, { enabled: isCustomer(c) });
  const bills = useApi<BillSummary[]>("bills.list", undefined, { enabled: isVendor(c) });

  useEffect(() => {
    // Let an open modal (e.g. Edit) take Escape first.
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && !document.querySelector(".modal-backdrop") && props.onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [props.onClose]);

  const myInvoices = (invoices.data ?? []).filter((i) => i.customer_id === c.id);
  const myBills = (bills.data ?? []).filter((b) => b.vendor_id === c.id);
  const owedToYou = myInvoices.filter((i) => i.status !== "void" && i.status !== "draft").reduce((s, i) => s + i.balance, 0);
  const youOwe = myBills.filter((b) => b.status !== "void").reduce((s, b) => s + b.balance, 0);

  const toggleArchive = async () => {
    try {
      await archive.mutateAsync({ id: c.id, archived: !c.archived });
      toast({ text: c.archived ? `${c.name} restored` : `${c.name} archived` });
    } catch (e) {
      toast.error(e);
    }
  };

  return (
    <>
      <div className="bk-panel-backdrop" onMouseDown={props.onClose} />
      <aside className="bk-panel" role="dialog" aria-modal="true" aria-label={c.name}>
        <div className="bk-panel-body stack" style={{ gap: 22 }}>
          <div className="row-between" style={{ alignItems: "flex-start" }}>
            <div className="row" style={{ gap: 14 }}>
              <div style={{ transform: "scale(1.25)", transformOrigin: "left center", marginRight: 8 }}>
                <Avatar name={c.name} />
              </div>
              <div style={{ minWidth: 0 }}>
                <div className="modal-title truncate">{c.name}</div>
                <div className="row" style={{ gap: 6, marginTop: 4 }}>
                  <span className={`pill ${KIND_PILL[c.kind]}`}>{KIND_LABEL[c.kind]}</span>
                  {c.archived && <span className="pill">Archived</span>}
                </div>
              </div>
            </div>
            <button className="btn btn-ghost btn-icon" onClick={props.onClose} aria-label="Close">
              <X size={18} />
            </button>
          </div>

          {(isCustomer(c) || isVendor(c)) && (
            <div className={isCustomer(c) && isVendor(c) ? "grid-2" : ""} style={{ gap: 12 }}>
              {isCustomer(c) && <BalanceTile label="Owes you" cents={owedToYou} color="var(--pos)" />}
              {isVendor(c) && <BalanceTile label="You owe" cents={youOwe} color="var(--warn)" />}
            </div>
          )}

          <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
            {isCustomer(c) && !c.archived && (
              <Link to={`/invoices/new?customer=${c.id}`} className="btn btn-primary btn-sm">
                <FileText size={14} /> New invoice
              </Link>
            )}
            {isVendor(c) && !c.archived && (
              <Link to={`/bills/new?vendor=${c.id}`} className={`btn ${isCustomer(c) ? "btn-secondary" : "btn-primary"} btn-sm`}>
                <Receipt size={14} /> Enter bill
              </Link>
            )}
            <button className="btn btn-secondary btn-sm" onClick={props.onEdit}>
              <Pencil size={14} /> Edit
            </button>
            <button className="btn btn-ghost btn-sm" onClick={toggleArchive} disabled={archive.isPending}>
              {c.archived ? <ArchiveRestore size={14} /> : <Archive size={14} />} {c.archived ? "Unarchive" : "Archive"}
            </button>
          </div>

          <section>
            <div className="label" style={{ marginBottom: 8 }}>
              Details
            </div>
            <div className="card" style={{ boxShadow: "none" }}>
              <Detail icon={<Users size={15} />} label="Company" value={c.company} />
              <Detail icon={<Mail size={15} />} label="Email" value={c.email && <a href={`mailto:${c.email}`}>{c.email}</a>} />
              <Detail icon={<Phone size={15} />} label="Phone" value={c.phone} />
              <Detail icon={<MapPin size={15} />} label="Address" value={c.address} />
              <Detail icon={<StickyNote size={15} />} label="Notes" value={c.notes} />
            </div>
          </section>

          {isCustomer(c) && (
            <DocList
              title="Invoices"
              loading={invoices.isLoading}
              empty="No invoices for this customer yet."
              rows={myInvoices.map((i) => ({
                id: i.id,
                to: `/invoices/${i.id}`,
                title: `Invoice ${i.number}`,
                date: i.issue_date,
                total: i.total,
                balance: i.balance,
                status: i.status,
              }))}
            />
          )}
          {isVendor(c) && (
            <DocList
              title="Bills"
              loading={bills.isLoading}
              empty="No bills from this vendor yet."
              rows={myBills.map((b) => ({
                id: b.id,
                to: `/bills/${b.id}`,
                title: b.number ? `Bill ${b.number}` : "Bill",
                date: b.issue_date,
                total: b.total,
                balance: b.balance,
                status: b.status,
              }))}
            />
          )}
        </div>
      </aside>
    </>
  );
}

function BalanceTile({ label, cents, color }: { label: string; cents: number; color: string }) {
  return (
    <div style={{ background: "var(--surface-2)", border: "1px solid var(--line)", borderRadius: 16, padding: "14px 16px" }}>
      <div className="label">{label}</div>
      <div className="num" style={{ fontSize: 24, fontWeight: 800, letterSpacing: "-.035em", marginTop: 2, color: cents ? color : undefined }}>
        {money(cents)}
      </div>
    </div>
  );
}

function Detail({ icon, label, value }: { icon: ReactNode; label: string; value: ReactNode }) {
  return (
    <div className="list-row" style={{ padding: "10px 16px", alignItems: "flex-start" }}>
      <span className="subtle" style={{ marginTop: 2 }}>
        {icon}
      </span>
      <span className="small muted" style={{ width: 70, flexShrink: 0 }}>
        {label}
      </span>
      <span className="grow" style={{ whiteSpace: "pre-line", minWidth: 0, overflowWrap: "anywhere" }}>
        {value || <span className="subtle">—</span>}
      </span>
    </div>
  );
}

type DocRow = { id: number; to: string; title: string; date: string; total: number; balance: number; status: InvoiceSummary["status"] };

function DocList(props: { title: string; loading: boolean; empty: string; rows: DocRow[] }) {
  return (
    <section>
      <div className="row-between" style={{ marginBottom: 8 }}>
        <span className="label">{props.title}</span>
        {props.rows.length > 0 && <span className="small subtle">{plural(props.rows.length, props.title.toLowerCase().slice(0, -1))}</span>}
      </div>
      <div className="card" style={{ boxShadow: "none" }}>
        {props.loading ? (
          <div className="list-row muted small">Loading…</div>
        ) : props.rows.length === 0 ? (
          <div className="list-row muted small" style={{ padding: "16px" }}>
            {props.empty}
          </div>
        ) : (
          props.rows.map((r) => (
            <Link key={r.id} to={r.to} className="list-row clickable" style={{ color: "inherit", padding: "11px 16px", opacity: r.status === "void" ? 0.5 : 1 }}>
              <div className="grow" style={{ minWidth: 0 }}>
                <div className="bold truncate">{r.title}</div>
                <div className="small subtle">{date(r.date)}</div>
              </div>
              <div style={{ textAlign: "right" }}>
                <Money cents={r.total} className="bold" />
                {r.balance > 0 && r.balance !== r.total && <div className="tiny subtle">{money(r.balance)} left</div>}
              </div>
              <StatusPill status={r.status} />
            </Link>
          ))
        )}
      </div>
    </section>
  );
}
