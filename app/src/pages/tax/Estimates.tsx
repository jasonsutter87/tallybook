// Quarterly estimated tax: a simple plan ("pay about $X per quarter"), the
// four due dates, payments recorded so far, and the full arithmetic behind
// the number. Tallybook prepares and explains; it never pays or files.

import { Ban, CalendarClock, CalendarX2, Check, ChevronRight, ExternalLink, Info, Pencil, Plus, Receipt, ShieldCheck } from "lucide-react";
import { useEffect, useMemo, useState, type ReactNode } from "react";
import { openExternal, useAction, useApi } from "../../api";
import {
  AccountSelect,
  BigMoney,
  Card,
  ConfirmModal,
  EmptyState,
  ErrorBanner,
  Field,
  Loading,
  Modal,
  MoneyInput,
  useAccountNames,
  useToast,
} from "../../components/ui";
import { date, money, shortDate, todayISO } from "../../format";
import type { Account, Cents, FilingStatus, Jurisdiction, QuarterPlan, TaxEstimate, TaxPayment, TaxProfile } from "../../types";
import type { TaxTabProps } from "../Taxes";
import "./tax-est.css";

const FILING: { value: FilingStatus; label: string }[] = [
  { value: "single", label: "Single" },
  { value: "married_joint", label: "Married filing jointly" },
  { value: "married_separate", label: "Married filing separately" },
  { value: "head_of_household", label: "Head of household" },
];
const filingLabel = (f: FilingStatus) => FILING.find((x) => x.value === f)?.label ?? f;

const METHOD_LABEL = { safe_harbor: "Safe harbor", current_year: "Based on this year" } as const;

const STATUS: Record<QuarterPlan["status"], [string, string]> = {
  paid: ["Paid", "pill-pos"],
  late: ["Past due", "pill-neg"],
  due_soon: ["Due soon", "pill-warn"],
  upcoming: ["Upcoming", "pill-brand"],
  nothing_due: ["Nothing due", ""],
};

const IRS_DIRECT_PAY = "https://www.irs.gov/payments/direct-pay";

/** 1986 → "19.9%", 500 → "5%". */
function pct(bp: number): string {
  return `${Number((bp / 100).toFixed(1))}%`;
}

export default function Estimates({ year, go }: TaxTabProps) {
  const est = useApi<TaxEstimate>("tax.estimate", { year });
  const profile = useApi<TaxProfile>("tax.profile");
  const [recording, setRecording] = useState<{ quarter: number } | null>(null);

  if (est.error?.code === "unsupported_year")
    return (
      <Card>
        <EmptyState icon={<CalendarX2 size={22} />} title={`No ${year} tax tables yet`} body={est.error.message} />
      </Card>
    );
  if (est.isLoading || profile.isLoading) return <Loading label="Working out your estimate…" />;
  if (est.error || profile.error || !est.data || !profile.data) return <ErrorBanner error={est.error ?? profile.error} />;

  const e = est.data;
  const p = profile.data;
  const needsSetup = p.method === "safe_harbor" && p.last_year_tax == null;
  const nextQuarter = e.quarters.find((q) => q.status === "late" || q.status === "due_soon" || q.status === "upcoming")?.quarter ?? null;

  return (
    <div className="stack" style={{ gap: 20 }}>
      <ProfileCard key={needsSetup ? "setup" : "set"} profile={p} needsSetup={needsSetup} />
      <PlanHero e={e} onRecord={() => setRecording({ quarter: nextQuarter ?? 4 })} />

      <div>
        <div className="grid-4">
          {e.quarters.map((q) => (
            <QuarterCard key={q.quarter} q={q} next={q.quarter === nextQuarter} onRecord={() => setRecording({ quarter: q.quarter })} />
          ))}
        </div>
        <div className="subtle small" style={{ marginTop: 10 }}>
          If a due date falls on a weekend or holiday, it moves to the next business day. Q4 is due in January of the following year.
        </div>
      </div>

      <div className="te-two-col">
        <HowCard e={e} go={go} />
        <History e={e} />
      </div>

      <RecordPaymentModal
        open={recording != null}
        onClose={() => setRecording(null)}
        year={year}
        quarters={e.quarters}
        stateQuarter={Math.round(e.state_set_aside / 4)}
        initialQuarter={recording?.quarter ?? 1}
      />
    </div>
  );
}

// ------------------------------------------------------------- profile

function ProfileCard({ profile, needsSetup }: { profile: TaxProfile; needsSetup: boolean }) {
  const [open, setOpen] = useState(needsSetup);
  if (!open)
    return (
      <div className="card card-pad te-profile-summary">
        <span className="icon-bubble">
          <ShieldCheck size={18} />
        </span>
        <div className="grow">
          <div className="bold">Your situation</div>
          <div className="muted small">
            {filingLabel(profile.filing_status)} · {METHOD_LABEL[profile.method]}
            {profile.method === "safe_harbor" && profile.last_year_tax != null && <> · last year's tax {money(profile.last_year_tax, { whole: true })}</>}
            {profile.other_income > 0 && <> · other income {money(profile.other_income, { whole: true })}</>}
            {profile.withholding > 0 && <> · withheld {money(profile.withholding, { whole: true })}</>}
            {profile.state_rate_bp > 0 && <> · state ~{pct(profile.state_rate_bp)}</>}
          </div>
        </div>
        <button className="btn btn-secondary btn-sm" onClick={() => setOpen(true)}>
          <Pencil size={14} /> Edit
        </button>
      </div>
    );
  return <ProfileForm profile={profile} firstRun={needsSetup} onDone={() => setOpen(false)} />;
}

function ProfileForm({ profile, firstRun, onDone }: { profile: TaxProfile; firstRun: boolean; onDone: () => void }) {
  const toast = useToast();
  const save = useAction<null, TaxProfile>("tax.set_profile");
  const [f, setF] = useState<TaxProfile>(profile);
  const [stateText, setStateText] = useState(profile.state_rate_bp ? String(profile.state_rate_bp / 100) : "");
  const [error, setError] = useState<Error | null>(null);
  const set = <K extends keyof TaxProfile>(k: K, v: TaxProfile[K]) => setF((x) => ({ ...x, [k]: v }));
  const hasExtras = profile.other_income > 0 || profile.w2_wages > 0 || profile.withholding > 0 || profile.projected_profit != null || profile.state_rate_bp > 0;

  const submit = async () => {
    const s = stateText.trim().replace(/%$/, "");
    const rate = s === "" ? 0 : Number(s);
    if (!Number.isFinite(rate) || rate < 0 || rate > 20) {
      setError(new Error("State rate should be a percent between 0 and 20 (e.g. 5)."));
      return;
    }
    if (f.method === "safe_harbor" && f.last_year_tax == null) {
      setError(new Error("Safe harbor needs last year's total tax. If you don't have it handy, choose “Based on this year” for now."));
      return;
    }
    try {
      await save.mutateAsync({ ...f, state_rate_bp: Math.round(rate * 100) });
      toast({ text: "Saved — your estimate is updated" });
      onDone();
    } catch (err) {
      setError(err as Error);
    }
  };

  return (
    <Card
      title={firstRun ? "Let's set up your estimate" : "Your situation"}
      action={!firstRun && <button className="btn btn-ghost btn-sm" onClick={onDone}>Cancel</button>}
    >
      <div className="stack" style={{ gap: 18 }}>
        {firstRun && (
          <div className="muted">
            A few answers about last year and your household let Tallybook suggest how much to pay each quarter. You can change these anytime.
          </div>
        )}
        <ErrorBanner error={error} />
        <div className="form-grid">
          <Field label="Filing status" hint="The status you expect to file with this year">
            <select className="select" value={f.filing_status} onChange={(ev) => set("filing_status", ev.target.value as FilingStatus)}>
              {FILING.map((x) => (
                <option key={x.value} value={x.value}>
                  {x.label}
                </option>
              ))}
            </select>
          </Field>
        </div>

        <div>
          <div className="label" style={{ marginBottom: 8 }}>
            How should we size your payments?
          </div>
          <div className="te-method-grid" role="radiogroup">
            <MethodCard
              on={f.method === "safe_harbor"}
              onClick={() => set("method", "safe_harbor")}
              title="Safe harbor (recommended)"
              body={
                <>
                  Pay at least last year's total tax (110% if last year's AGI was over $150,000, or $75,000 if married filing separately) in four
                  equal parts and you won't owe an underpayment penalty, even if you earn more this year.
                </>
              }
            />
            <MethodCard
              on={f.method === "current_year"}
              onClick={() => set("method", "current_year")}
              title="Based on this year"
              body="Estimates this year's tax from your books. Paying at least 90% of this year's tax also avoids the penalty, but the number moves as your income changes."
            />
          </div>
        </div>

        {f.method === "safe_harbor" && (
          <div className="form-grid">
            <Field label="Last year's total tax — Form 1040, “total tax” line" hint="From last year's return. Not the refund or amount owed.">
              <MoneyInput value={f.last_year_tax} onChange={(v) => set("last_year_tax", v)} placeholder="e.g. 18,400.00" />
            </Field>
            <Field label="Last year's adjusted gross income — Form 1040 AGI line" hint="Decides whether the 110% rule applies">
              <MoneyInput value={f.last_year_agi} onChange={(v) => set("last_year_agi", v)} placeholder="e.g. 96,000.00" />
            </Field>
          </div>
        )}

        <details className="te-disclosure" open={hasExtras || undefined}>
          <summary>
            <ChevronRight size={14} className="te-chev" /> More about your situation
          </summary>
          <div className="form-grid" style={{ marginTop: 14 }}>
            <Field label="Other income this year" hint="W-2 wages, a spouse's income (if filing jointly), interest…">
              <MoneyInput value={f.other_income} onChange={(v) => set("other_income", v ?? 0)} />
            </Field>
            <Field label="W-2 Social Security wages" hint="Box 3 of your W-2s. Lowers the Social Security part of self-employment tax if you're near the cap.">
              <MoneyInput value={f.w2_wages} onChange={(v) => set("w2_wages", v ?? 0)} />
            </Field>
            <Field label="Federal tax withheld from paychecks" hint="Expected for the full year. Counts toward what you owe.">
              <MoneyInput value={f.withholding} onChange={(v) => set("withholding", v ?? 0)} />
            </Field>
            <Field label="Projected business profit" hint="Leave blank to use your books">
              <MoneyInput value={f.projected_profit} onChange={(v) => set("projected_profit", v)} placeholder="From your books" />
            </Field>
            <Field label="Rough state income-tax rate" hint="Optional. Only used for a “set aside for state” tip — Tallybook doesn't calculate state tax.">
              <div className="te-suffix">
                <input className="input num" inputMode="decimal" placeholder="0" value={stateText} onChange={(ev) => setStateText(ev.target.value)} />
                <span>%</span>
              </div>
            </Field>
          </div>
        </details>

        <div className="row" style={{ justifyContent: "flex-end" }}>
          {!firstRun && (
            <button className="btn btn-secondary" onClick={onDone}>
              Cancel
            </button>
          )}
          <button className="btn btn-primary" onClick={submit} disabled={save.isPending}>
            {save.isPending ? "Saving…" : firstRun ? "Show my plan" : "Save"}
          </button>
        </div>
      </div>
    </Card>
  );
}

function MethodCard(props: { on: boolean; onClick: () => void; title: string; body: ReactNode }) {
  return (
    <button type="button" role="radio" aria-checked={props.on} className={`te-method ${props.on ? "on" : ""}`} onClick={props.onClick}>
      <span className="te-method-title">
        <span className="te-radio" />
        {props.title}
      </span>
      <span className="te-method-body">{props.body}</span>
    </button>
  );
}

// ------------------------------------------------------------- hero

function PlanHero({ e, onRecord }: { e: TaxEstimate; onRecord: () => void }) {
  const typical = Math.max(0, ...e.quarters.map((q) => q.planned));
  const paidFederal = e.payments.filter((p) => !p.voided && p.jurisdiction === "federal").reduce((n, p) => n + p.amount, 0);
  const progress = e.plan_annual > 0 ? Math.min(100, (paidFederal / e.plan_annual) * 100) : 100;
  const fellBack = e.profile.method === "safe_harbor" && e.plan_method === "current_year";

  return (
    <div className="hero">
      <div className="te-hero-row">
        <div>
          <div className="hero-label">Your {e.year} federal estimated tax plan · an estimate</div>
          {e.plan_annual > 0 ? (
            <div className="hero-amount">
              <span style={{ fontSize: "0.55em", fontWeight: 700, letterSpacing: "-.02em", marginRight: 8 }}>Pay about</span>
              <BigMoney cents={typical} />
              <span className="te-hero-per">per quarter</span>
            </div>
          ) : (
            <div className="hero-amount" style={{ fontSize: 34 }}>
              Nothing to pay in estimates
            </div>
          )}
          <div className="te-hero-chips">
            <span className="te-hero-chip">
              <span>Method</span> {METHOD_LABEL[e.plan_method]}
            </span>
            <span className="te-hero-chip">
              <span>For the year</span> {money(e.plan_annual)}
            </span>
            {e.estimate.set_aside_bp > 0 && (
              <span className="te-hero-chip" title="Your estimated federal tax as a share of business profit. Moving this much to savings as money comes in keeps tax time calm.">
                Set aside {pct(e.estimate.set_aside_bp)} of profit
              </span>
            )}
            {e.state_set_aside > 0 && (
              <span className="te-hero-chip" title={`About ${pct(e.profile.state_rate_bp)} of profit, from the rough state rate you entered. Not a state tax calculation.`}>
                + about {money(e.state_set_aside, { whole: true })} for state
              </span>
            )}
          </div>
          {fellBack && (
            <div className="small" style={{ marginTop: 12, color: "var(--hero-ink-2)", fontWeight: 600 }}>
              Using this year's books until you add last year's total tax above.
            </div>
          )}
        </div>
        <button className="btn btn-on-hero" onClick={onRecord}>
          <Plus size={16} /> Record a payment
        </button>
      </div>
      {e.plan_annual > 0 && (
        <div className="te-hero-progress">
          <div className="te-hero-foot">
            <span>
              Paid so far <b>{money(paidFederal)}</b>
            </span>
            <span>
              {paidFederal >= e.plan_annual ? "Plan covered" : <>{money(e.plan_annual - paidFederal)} to go</>}
            </span>
          </div>
          <div className="te-bar">
            <div style={{ width: `${progress}%` }} />
          </div>
        </div>
      )}
    </div>
  );
}

// ------------------------------------------------------------- quarters

function QuarterCard({ q, next, onRecord }: { q: QuarterPlan; next: boolean; onRecord: () => void }) {
  const [label, cls] = STATUS[q.status];
  const paidPct = q.planned > 0 ? Math.min(100, (q.paid_federal / q.planned) * 100) : 0;
  return (
    <div className={`card card-pad te-quarter ${next ? "next" : ""}`}>
      <div className="row-between">
        <span className="te-q-label">Q{q.quarter}</span>
        <span className={`pill ${cls}`}>
          {q.status === "paid" && <Check size={12} />}
          {label}
        </span>
      </div>
      <div className="row subtle small" style={{ gap: 6, marginTop: 4 }}>
        <CalendarClock size={13} /> Due {date(q.due)}
      </div>
      <div className="te-q-amount num">
        <BigMoney cents={q.planned} />
      </div>
      <div className="subtle small">planned</div>
      {q.planned > 0 && (
        <div className="te-q-bar" aria-hidden>
          <div style={{ width: `${paidPct}%` }} />
        </div>
      )}
      <div className="te-q-paid">
        <span>Paid federal</span>
        <span className="num bold">{money(q.paid_federal)}</span>
      </div>
      {q.paid_state > 0 && (
        <div className="te-q-paid">
          <span>Paid state</span>
          <span className="num bold">{money(q.paid_state)}</span>
        </div>
      )}
      {q.status === "late" && <div className="small muted" style={{ marginTop: 6 }}>Already paid? Record it so your plan stays accurate.</div>}
      <div className="te-q-foot">
        <button className={`btn btn-sm btn-block ${next ? "btn-primary" : "btn-secondary"}`} onClick={onRecord}>
          Record payment
        </button>
      </div>
    </div>
  );
}

// ------------------------------------------------------------- show the work

function Step(props: { op?: string; label: ReactNode; detail?: ReactNode; amount: Cents; neg?: boolean; variant?: "sub" | "total" }) {
  return (
    <div className={`te-step ${props.variant ?? ""}`}>
      <span className="te-step-op">{props.op ?? ""}</span>
      <div>
        <div className="te-step-label">{props.label}</div>
        {props.detail && <div className="te-step-detail">{props.detail}</div>}
      </div>
      <span className="te-step-amount num">{props.neg ? `−${money(props.amount)}` : money(props.amount)}</span>
    </div>
  );
}

function HowCard({ e, go }: { e: TaxEstimate; go: TaxTabProps["go"] }) {
  const d = e.estimate;
  const p = e.profile;
  const usingBooks = p.projected_profit == null;
  return (
    <Card title="How we got this number" action={<span className="pill">Estimate</span>}>
      <div className="te-work">
        <div className="te-subsection" style={{ marginTop: 0 }}>
          Business profit
        </div>
        <Step
          label="Profit used for the year"
          detail={
            <>
              Based on {e.profit_basis}.{" "}
              {usingBooks && (
                <button type="button" className="te-link" onClick={() => go("schedule-c")}>
                  See your Schedule C
                </button>
              )}
            </>
          }
          amount={e.profit_used}
        />
        {usingBooks && (
          <>
            <Step variant="sub" label="Profit so far this year" amount={e.profit_to_date} />
            <Step variant="sub" label="Extended to a full year" amount={e.profit_annualized} />
          </>
        )}

        <div className="te-subsection">Self-employment tax (Social Security + Medicare)</div>
        <Step variant="sub" label="Net earnings" detail="92.35% of profit — the IRS taxes slightly less than all of it" amount={d.se.net_earnings} />
        <Step variant="sub" op="+" label="Social Security · 12.4%" detail="Up to the yearly wage cap, less any W-2 Social Security wages" amount={d.se.social_security} />
        <Step variant="sub" op="+" label="Medicare · 2.9%" amount={d.se.medicare} />
        <Step label="Self-employment tax" amount={d.se.total} />
        {d.additional_medicare > 0 && <Step op="+" label="Additional Medicare tax · 0.9%" detail="On earnings above the threshold for your filing status" amount={d.additional_medicare} />}

        <div className="te-subsection">Income tax</div>
        <Step variant="sub" label="Business profit" amount={e.profit_used} />
        {p.other_income > 0 && <Step variant="sub" op="+" label="Other income" amount={p.other_income} />}
        <Step variant="sub" op="−" label="Half of self-employment tax" detail="You get to deduct half — it's what an employer would pay" amount={d.se.deductible_half} neg />
        <Step label="Adjusted gross income (AGI)" amount={d.agi} />
        <Step variant="sub" op="−" label="Standard deduction" detail={filingLabel(p.filing_status)} amount={d.standard_deduction} neg />
        {d.qbi_limited ? (
          <div className="te-note" style={{ margin: "6px 0" }}>
            <Info size={15} />
            <span>Your income is above the simplified limit, so we left the QBI deduction out — a pro can tell you if you qualify.</span>
          </div>
        ) : (
          <Step
            variant="sub"
            op="−"
            label="Qualified business income (QBI) deduction"
            detail="Up to 20% of business profit for many small businesses"
            amount={d.qbi_deduction}
            neg
          />
        )}
        <Step label="Taxable income" amount={d.taxable_income} />
        <Step label="Income tax" detail="From this year's federal tax brackets" amount={d.income_tax} />

        <div className="te-subsection">Putting it together</div>
        <Step variant="sub" label="Income tax" amount={d.income_tax} />
        <Step variant="sub" op="+" label="Self-employment tax" amount={d.se.total} />
        {d.additional_medicare > 0 && <Step variant="sub" op="+" label="Additional Medicare tax" amount={d.additional_medicare} />}
        {d.withholding > 0 ? (
          <>
            <Step label="Estimated total federal tax for the year" amount={d.total_tax} />
            <Step variant="sub" op="−" label="Already withheld from paychecks" amount={d.withholding} neg />
            <Step variant="total" op="=" label="Left to cover with estimates, by your books" amount={d.to_pay} />
          </>
        ) : (
          <Step variant="total" op="=" label="Estimated total federal tax for the year" amount={d.total_tax} />
        )}
      </div>

      <div className="te-subsection" style={{ marginTop: 22 }}>
        Two ways to avoid the underpayment penalty
      </div>
      <div className="te-compare">
        <div className={`te-compare-box ${e.plan_method === "safe_harbor" ? "on" : ""}`}>
          <div className="bold small">Safe harbor</div>
          {e.safe_harbor != null ? (
            <div className="num">{money(e.safe_harbor)}</div>
          ) : (
            <div className="subtle small" style={{ marginTop: 6 }}>
              Add last year's total tax to see this.
            </div>
          )}
          <div className="subtle small">100% of last year's tax (110% above the AGI limit). Stays put all year.</div>
        </div>
        <div className={`te-compare-box ${e.plan_method === "current_year" ? "on" : ""}`}>
          <div className="bold small">90% of this year</div>
          <div className="num">{money(e.ninety_percent_current)}</div>
          <div className="subtle small">Moves as your income changes.</div>
        </div>
      </div>
      <div className="subtle small" style={{ marginTop: 10 }}>
        Your plan uses <b>{METHOD_LABEL[e.plan_method].toLowerCase()}</b>
        {d.withholding > 0 && <>, minus paycheck withholding</>}: {money(e.plan_annual)} for the year. Meeting either target is generally enough; paying
        less than both can mean a penalty.
        {e.plan_method === "safe_harbor" && e.safe_harbor != null && e.ninety_percent_current < e.safe_harbor && (
          <>
            {" "}
            Your books currently point lower than safe harbor. “Based on this year” would mean smaller payments, but they'd change as your income does.
          </>
        )}
      </div>

      {d.not_included.length > 0 && (
        <div className="te-note" style={{ marginTop: 16, flexDirection: "column", gap: 4 }}>
          <span className="bold" style={{ color: "var(--ink)" }}>
            Not included in this estimate
          </span>
          <ul className="te-list">
            {d.not_included.map((x) => (
              <li key={x}>{x}</li>
            ))}
          </ul>
        </div>
      )}
      <div className="subtle tiny" style={{ marginTop: 12 }}>
        Tax tables: {d.source}
      </div>
    </Card>
  );
}

// ------------------------------------------------------------- history

function History({ e }: { e: TaxEstimate }) {
  const toast = useToast();
  const name = useAccountNames();
  const voidPayment = useAction<null, { id: number }>("tax.void_payment");
  const [voiding, setVoiding] = useState<TaxPayment | null>(null);
  const rows = [...e.payments].sort((a, b) => (a.date < b.date ? 1 : a.date > b.date ? -1 : b.id - a.id));

  return (
    <Card title="Payments recorded" pad={false}>
      <div style={{ paddingTop: 10, paddingBottom: 6 }}>
        {rows.length === 0 ? (
          <EmptyState icon={<Receipt size={22} />} title="No payments yet" body="When you pay the IRS (or your state), record it here so your plan keeps track." />
        ) : (
          rows.map((p) => (
            <div key={p.id} className={`te-hist-row ${p.voided ? "voided" : ""}`}>
              <div className="grow">
                <div className="row" style={{ gap: 6 }}>
                  <span className="bold">Q{p.quarter}</span>
                  <span className={`pill ${p.jurisdiction === "federal" ? "pill-brand" : "pill-violet"}`}>{p.jurisdiction === "federal" ? "Federal" : "State"}</span>
                  {p.voided && <span className="pill">Voided</span>}
                </div>
                <div className="subtle small truncate">
                  {shortDate(p.date)} · from {name(p.account_id)}
                </div>
              </div>
              <span className="te-hist-amount num">{money(p.amount)}</span>
              {!p.voided && (
                <button className="btn btn-ghost btn-icon" title="Void this payment" aria-label="Void this payment" onClick={() => setVoiding(p)}>
                  <Ban size={15} />
                </button>
              )}
            </div>
          ))
        )}
      </div>
      <div className="te-hist-row subtle small" style={{ borderTop: "1px solid var(--line)" }}>
        <Info size={15} style={{ flexShrink: 0 }} />
        <span>Recorded as owner's draws, so they don't count as business expenses.</span>
      </div>
      <ConfirmModal
        open={voiding != null}
        onClose={() => setVoiding(null)}
        title="Void this payment?"
        confirmLabel="Void payment"
        danger
        body={
          voiding && (
            <>
              This removes the {money(voiding.amount)} Q{voiding.quarter} {voiding.jurisdiction} payment from your plan and reverses its entry in your books.
              It doesn't cancel anything with the IRS or your bank.
            </>
          )
        }
        onConfirm={async () => {
          if (!voiding) return;
          try {
            await voidPayment.mutateAsync({ id: voiding.id });
            toast({ text: "Payment voided" });
          } catch (err) {
            toast.error(err);
          }
        }}
      />
    </Card>
  );
}

// ------------------------------------------------------------- record payment

function RecordPaymentModal(props: { open: boolean; onClose: () => void; year: number; quarters: QuarterPlan[]; stateQuarter: Cents; initialQuarter: number }) {
  const toast = useToast();
  const accounts = useApi<Account[]>("accounts.list");
  const record = useAction<number, { tax_year: number; quarter: number; jurisdiction: Jurisdiction; date: string; amount: Cents; account_id: number }>(
    "tax.record_payment",
  );
  const firstBank = useMemo(() => accounts.data?.find((a) => a.subtype === "bank" && !a.archived)?.id ?? null, [accounts.data]);

  const [quarter, setQuarter] = useState(props.initialQuarter);
  const [jur, setJur] = useState<Jurisdiction>("federal");
  const [day, setDay] = useState(todayISO());
  const [amount, setAmount] = useState<Cents | null>(null);
  const [accountId, setAccountId] = useState<number | null>(firstBank);
  const [error, setError] = useState<Error | null>(null);

  const defaults = (qn: number, j: Jurisdiction) => {
    const q = props.quarters.find((x) => x.quarter === qn);
    if (!q) return;
    const today = todayISO();
    setDay(q.due < today ? q.due : today);
    const left = j === "federal" ? q.planned - q.paid_federal : props.stateQuarter - q.paid_state;
    setAmount(left > 0 ? left : null);
  };

  useEffect(() => {
    if (!props.open) return;
    setQuarter(props.initialQuarter);
    setJur("federal");
    setError(null);
    defaults(props.initialQuarter, "federal");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.open, props.initialQuarter]);

  useEffect(() => {
    if (accountId == null && firstBank != null) setAccountId(firstBank);
  }, [firstBank, accountId]);

  const submit = async () => {
    if (amount == null || amount <= 0) return setError(new Error("Enter how much you paid."));
    if (accountId == null) return setError(new Error("Choose the account you paid from."));
    if (!day) return setError(new Error("Enter the date you paid."));
    try {
      await record.mutateAsync({ tax_year: props.year, quarter, jurisdiction: jur, date: day, amount, account_id: accountId });
      toast({ text: `Recorded ${money(amount)} for Q${quarter}` });
      props.onClose();
    } catch (err) {
      setError(err as Error);
    }
  };

  const q = props.quarters.find((x) => x.quarter === quarter);

  return (
    <Modal
      open={props.open}
      onClose={props.onClose}
      title="Record an estimated tax payment"
      subtitle="For a payment you've already made. Tallybook doesn't send money anywhere."
      footer={
        <>
          <button className="btn btn-secondary" onClick={props.onClose}>
            Cancel
          </button>
          <button className="btn btn-primary" onClick={submit} disabled={record.isPending}>
            {record.isPending ? "Saving…" : "Record payment"}
          </button>
        </>
      }
    >
      <div className="stack">
        <ErrorBanner error={error} />
        <div className="form-grid">
          <Field label="Quarter" hint={q ? `Due ${date(q.due)}` : undefined}>
            <select
              className="select"
              value={quarter}
              onChange={(ev) => {
                const n = Number(ev.target.value);
                setQuarter(n);
                defaults(n, jur);
              }}
            >
              {props.quarters.map((x) => (
                <option key={x.quarter} value={x.quarter}>
                  Q{x.quarter} · due {shortDate(x.due)}
                </option>
              ))}
            </select>
          </Field>
          <div className="field">
            <span className="label">Paid to</span>
            <div className="te-seg" role="radiogroup">
              {(["federal", "state"] as const).map((j) => (
                <button
                  key={j}
                  type="button"
                  role="radio"
                  aria-checked={jur === j}
                  className={jur === j ? "on" : ""}
                  onClick={() => {
                    setJur(j);
                    defaults(quarter, j);
                  }}
                >
                  {j === "federal" ? "IRS (federal)" : "State"}
                </button>
              ))}
            </div>
          </div>
          <Field label="Date paid">
            <input className="input" type="date" value={day} onChange={(ev) => setDay(ev.target.value)} />
          </Field>
          <Field label="Amount" hint={jur === "federal" && q ? `Planned ${money(q.planned)} · paid ${money(q.paid_federal)}` : undefined}>
            <MoneyInput value={amount} onChange={setAmount} />
          </Field>
          <Field label="Paid from" className="span-2">
            <AccountSelect value={accountId} onChange={setAccountId} subtypes={["bank", "credit_card"]} placeholder="Choose a bank account or card" />
          </Field>
        </div>
        <div className="te-note">
          <Info size={15} />
          <span>Recorded as an owner's draw — estimated income tax is personal, not a business expense.</span>
        </div>
        <div className="small muted">
          Haven't paid yet? You can pay the IRS online with{" "}
          <a
            className="te-link"
            href={IRS_DIRECT_PAY}
            onClick={(ev) => {
              ev.preventDefault();
              void openExternal(IRS_DIRECT_PAY);
            }}
          >
            IRS Direct Pay <ExternalLink size={12} />
          </a>
          , then come back and record it.
        </div>
      </div>
    </Modal>
  );
}
