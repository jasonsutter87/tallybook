// 1099-NEC prep: who you paid enough to need a form, whether you have their
// W-9, and how to file. Tallybook totals and explains; you (or your
// preparer) file.

import { AlertTriangle, CalendarX2, ExternalLink, FileCheck2, FileWarning, Pencil, Search, Users } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router-dom";
import { openExternal, useAction, useApi } from "../../api";
import { Card, EmptyState, ErrorBanner, Loading, Money, useToast } from "../../components/ui";
import { ContactTaxModal } from "../../flows/ContactTaxFields";
import { money } from "../../format";
import type { ContactTaxInput, Payee1099, Report1099 } from "../../types";
import type { TaxTabProps } from "../Taxes";
import "./tax-est.css";

const IRIS = "https://www.irs.gov/filing/e-file-information-returns-with-iris";

function ExternalLinkText({ url, children }: { url: string; children: React.ReactNode }) {
  return (
    <a
      className="te-link"
      href={url}
      onClick={(ev) => {
        ev.preventDefault();
        void openExternal(url);
      }}
    >
      {children} <ExternalLink size={12} />
    </a>
  );
}

const taxOf = (r: Payee1099): ContactTaxInput => ({ is_1099: r.is_1099, w9_on_file: r.w9_on_file, legal_name: r.legal_name, tin_last4: r.tin_last4 });

export default function Ten99({ year, go }: TaxTabProps) {
  const toast = useToast();
  const report = useApi<Report1099>("tax.1099", { year });
  const setContact = useAction<null, { id: number } & ContactTaxInput>("tax.set_contact");
  const [editing, setEditing] = useState<Payee1099 | null>(null);

  if (report.error?.code === "unsupported_year")
    return (
      <Card>
        <EmptyState icon={<CalendarX2 size={22} />} title={`No ${year} tax tables yet`} body={report.error.message} />
      </Card>
    );
  if (report.isLoading) return <Loading />;
  if (report.error || !report.data) return <ErrorBanner error={report.error} />;

  const { threshold, rows } = report.data;
  const t = money(threshold, { whole: true });
  const needed = rows.filter((r) => r.over_threshold);
  const missingW9 = needed.filter((r) => !r.w9_on_file);
  const check = rows.filter((r) => !r.is_1099 && r.paid_reportable >= threshold);
  const sorted = [...rows].sort((a, b) => Number(b.over_threshold) - Number(a.over_threshold) || b.paid_reportable - a.paid_reportable);

  const toggleContractor = async (r: Payee1099) => {
    try {
      await setContact.mutateAsync({ id: r.contact_id, ...taxOf(r), is_1099: !r.is_1099 });
      toast({ text: r.is_1099 ? `${r.name} is no longer marked as a contractor` : `${r.name} marked as a 1099 contractor` });
    } catch (e) {
      toast.error(e);
    }
  };

  return (
    <div className="stack" style={{ gap: 20 }}>
      <div className="card card-pad row" style={{ gap: 14, alignItems: "flex-start" }}>
        <span className="icon-bubble">
          <Users size={18} />
        </span>
        <div className="grow">
          <div className="bold">Who needs a 1099-NEC?</div>
          <div className="muted" style={{ marginTop: 2 }}>
            Pay a contractor (not a corporation) {t} or more in a year by check, ACH, Zelle, cash, etc.? You'll likely need to send them a 1099-NEC by
            January 31. Card and PayPal payments don't count — those networks report them.
          </div>
        </div>
      </div>

      <div className="grid-3">
        <SummaryCard icon={<FileCheck2 size={18} />} tone="" value={needed.length} label="Forms likely needed" foot={`Contractors paid ${t}+ from the bank in ${year}`} />
        <SummaryCard
          icon={<FileWarning size={18} />}
          tone={missingW9.length ? "neg" : "pos"}
          value={missingW9.length}
          label="W-9s missing"
          foot={missingW9.length ? "Ask for these before filing — you need their tax ID" : "Every form has a W-9 on file"}
          alert={missingW9.length > 0}
        />
        <SummaryCard
          icon={<Search size={18} />}
          tone={check.length ? "warn" : ""}
          value={check.length}
          label="Check these"
          foot={check.length ? `Paid ${t}+ but not marked as contractors` : "Nobody unexpected over the limit"}
        />
      </div>

      {rows.length === 0 ? (
        <Card>
          <EmptyState
            icon={<Users size={22} />}
            title="No one to list yet"
            body={
              <>
                Tallybook totals payments that are tagged with who you paid. Bill payments are tagged automatically. For transfers, checks and Zelle from your
                bank, open <b>For review</b> and choose <b>Paid to</b> while categorizing (or set it on any transaction in an account register).
              </>
            }
            action={
              <Link to="/review" className="btn btn-primary">
                Go to For review
              </Link>
            }
          />
        </Card>
      ) : (
        <Card title={`Payees in ${year}`} pad={false}>
          <div className="te-table-wrap" style={{ marginTop: 8 }}>
            <table className="table te-table">
              <thead>
                <tr>
                  <th>Payee</th>
                  <th>Legal name</th>
                  <th>Tax ID</th>
                  <th>W-9</th>
                  <th className="right">Paid from bank</th>
                  <th className="right">Paid by card</th>
                  <th>Status</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {sorted.map((r) => (
                  <tr key={r.contact_id}>
                    <td>
                      <Link to={`/contacts?contact=${r.contact_id}`} className="bold">
                        {r.name}
                      </Link>
                    </td>
                    <td>{r.legal_name ?? <span className="subtle">Not entered</span>}</td>
                    <td className="num nowrap">{r.tin_last4 ? `···${r.tin_last4}` : <span className="subtle">—</span>}</td>
                    <td>
                      {r.w9_on_file ? (
                        <span className="pill pill-pos">On file</span>
                      ) : (
                        <span className={`pill ${r.over_threshold ? "pill-neg" : ""}`}>Missing</span>
                      )}
                    </td>
                    <td className="right">
                      <Money cents={r.paid_reportable} className="bold" />
                    </td>
                    <td className="right" title="Not reportable by you — the card network or PayPal reports these">
                      <Money cents={r.paid_by_card} className="subtle" />
                      {r.paid_by_card > 0 && <div className="subtle tiny">not reportable by you</div>}
                    </td>
                    <td>
                      <StatusPill r={r} threshold={threshold} />
                    </td>
                    <td className="right">
                      <div className="row" style={{ justifyContent: "flex-end", gap: 4 }}>
                        <button className="btn btn-ghost btn-sm" onClick={() => toggleContractor(r)} disabled={setContact.isPending}>
                          {r.is_1099 ? "Unmark" : "Mark as contractor"}
                        </button>
                        <button className="btn btn-ghost btn-icon" title="Edit tax details" aria-label={`Edit tax details for ${r.name}`} onClick={() => setEditing(r)}>
                          <Pencil size={15} />
                        </button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div className="subtle small" style={{ padding: "12px 22px 16px", borderTop: "1px solid var(--line)" }}>
            Only payments tagged with a payee are counted. Corporations usually don't get a 1099-NEC (attorneys are an exception) — their W-9 says which
            they are.
          </div>
        </Card>
      )}

      <Card title="How to file">
        <ol className="te-steps">
          <li>
            <div>
              <div className="bold">Collect a W-9 from each contractor</div>
              <div className="muted small">It gives you their legal name, address and tax ID. Keep it in your records; Tallybook only stores the last four digits.</div>
            </div>
          </li>
          <li>
            <div>
              <div className="bold">File Form 1099-NEC with the IRS by January 31</div>
              <div className="muted small">
                You can file free through the IRS <ExternalLinkText url={IRIS}>IRIS portal</ExternalLinkText>, or have your tax preparer do it. Your state may
                want a copy too.
              </div>
            </div>
          </li>
          <li>
            <div>
              <div className="bold">Send Copy B to each contractor by January 31</div>
              <div className="muted small">By mail, or by email if they've agreed to get it electronically.</div>
            </div>
          </li>
        </ol>
        <div className="row-between" style={{ marginTop: 20, paddingTop: 16, borderTop: "1px solid var(--line)", flexWrap: "wrap" }}>
          <span className="muted small">The summary includes each payee's totals from bank and card, ready for you or your preparer.</span>
          <button className="btn btn-secondary btn-sm" onClick={() => go("export")}>
            Export the 1099 summary
          </button>
        </div>
      </Card>

      <ContactTaxModal
        open={editing != null}
        onClose={() => setEditing(null)}
        contactId={editing?.contact_id ?? null}
        name={editing?.name ?? ""}
        initial={editing ? taxOf(editing) : { is_1099: false, w9_on_file: false, legal_name: null, tin_last4: null }}
      />
    </div>
  );
}

function StatusPill({ r, threshold }: { r: Payee1099; threshold: number }) {
  if (!r.is_1099 && r.paid_reportable >= threshold)
    return (
      <span className="pill pill-warn" title="You paid them enough that a 1099 might be needed. If they're a contractor (not a corporation), mark them.">
        <AlertTriangle size={12} /> Not marked as contractor — check
      </span>
    );
  if (r.over_threshold) return <span className="pill pill-brand">Needs 1099</span>;
  return <span className="pill">Under threshold</span>;
}

function SummaryCard(props: { icon: React.ReactNode; tone: string; value: number; label: string; foot: string; alert?: boolean }) {
  return (
    <div className={`card card-pad te-stat ${props.alert ? "alert" : ""}`}>
      <span className={`icon-bubble ${props.tone}`}>{props.icon}</span>
      <div>
        <div className="stat-label">{props.label}</div>
        <div className={`te-stat-value num ${props.alert ? "neg" : ""}`} style={{ marginTop: 4 }}>
          {props.value}
        </div>
        <div className="stat-foot">{props.foot}</div>
      </div>
    </div>
  );
}
