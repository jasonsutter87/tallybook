import { ArrowRight, CalendarClock, Check, CircleSlash, FileSpreadsheet, ListChecks, PiggyBank, UserCog, Users } from "lucide-react";
import type { ReactNode } from "react";
import { useApi } from "../../api";
import { BigMoney, Card, Loading } from "../../components/ui";
import { date, money, plural, todayISO } from "../../format";
import type { QuarterPlan, Report1099, ScheduleCResponse, TaxEstimate } from "../../types";
import type { TaxTabProps } from "../Taxes";
import "./tax.css";
import { TaxError, firstError } from "./txShared";

const QUARTER_PILL: Record<QuarterPlan["status"], [string, string]> = {
  late: ["Late", "pill-neg"],
  due_soon: ["Due soon", "pill-warn"],
  upcoming: ["Upcoming", "pill-brand"],
  paid: ["Paid", "pill-pos"],
  nothing_due: ["Nothing due", ""],
};

/** 1986 basis points → "20%" (we say "about", so whole percents read better). */
function pct(bp: number): string {
  return `${Math.round(bp / 100)}%`;
}

export default function Overview({ year, go }: TaxTabProps) {
  const sc = useApi<ScheduleCResponse>("tax.schedule_c", { year });
  const est = useApi<TaxEstimate>("tax.estimate", { year });
  const t99 = useApi<Report1099>("tax.1099", { year });

  const error = firstError(sc.error, est.error, t99.error);
  if (error) return <TaxError error={error} year={year} />;
  if (!sc.data || !est.data || !t99.data) return <Loading />;

  const { report } = sc.data;
  const s = report.schedule;
  const e = est.data;
  const thisYear = Number(todayISO().slice(0, 4));
  const inProgress = year >= thisYear;

  // Checklist inputs.
  const unconfirmed = report.unconfirmed;
  const profileDone = e.profile.last_year_tax != null;
  const nextQuarter = e.quarters.find((q) => q.status !== "paid" && q.status !== "nothing_due");
  const needForm = t99.data.rows.filter((r) => r.over_threshold).length;
  const maybeForm = t99.data.rows.filter((r) => !r.is_1099 && r.paid_reportable >= t99.data.threshold).length;

  return (
    <div className="stack" style={{ gap: 20 }}>
      {/* Hero: the year's bottom line */}
      <div className="hero">
        <div className="row-between" style={{ alignItems: "flex-start" }}>
          <div>
            <div className="hero-label">
              Estimated net profit · Schedule C line 31 · {year}
              {inProgress ? " so far" : ""}
            </div>
            <div className="hero-amount">
              <BigMoney cents={s.line31_net_profit} />
            </div>
            <div style={{ marginTop: 8, color: "var(--hero-ink-2)", fontWeight: 600, fontSize: 13 }}>
              {money(s.line7_gross_income)} gross income − {money(s.line28_total_expenses + s.line30_home_office)} expenses
              {inProgress && e.profit_used !== e.profit_to_date && (
                <>
                  {" "}
                  · on pace for about <span style={{ color: "#fff" }}>{money(e.profit_used, { whole: true })}</span> ({e.profit_basis})
                </>
              )}
            </div>
          </div>
          <button className="btn btn-on-hero btn-sm" style={{ position: "relative", zIndex: 1 }} onClick={() => go("schedule-c")}>
            <FileSpreadsheet size={14} /> See the worksheet
          </button>
        </div>
        <div className="tx-hero-chips">
          <span className="tx-hero-chip">
            <PiggyBank size={15} />
            Set aside about {pct(e.estimate.set_aside_bp)} for federal taxes
            {e.state_set_aside > 0 && <span className="dim">+ about {pct(e.profile.state_rate_bp)} for state</span>}
          </span>
          <span className="tx-hero-chip">
            Estimated self-employment tax <span>{money(e.estimate.se.total, { whole: true })}</span>
            <span className="dim">· Social Security + Medicare</span>
          </span>
        </div>
      </div>

      <div className="tx-overview-grid">
        <Card title="Your tax checklist" action={<span className="subtle small">Four steps, in any order</span>} pad={false}>
          <div style={{ paddingTop: 8 }}>
            <Step
              n={1}
              icon={<ListChecks size={18} />}
              done={unconfirmed === 0}
              title="Confirm your Schedule C categories"
              detail={
                unconfirmed === 0
                  ? "Every account with activity is matched to a Schedule C line."
                  : "Tallybook suggested a line for each account. A quick look makes sure each one lands in the right place."
              }
              status={unconfirmed === 0 ? <span className="pill pill-pos">Done</span> : <span className="pill pill-warn">{plural(unconfirmed, "account")} to review</span>}
              action={unconfirmed === 0 ? "View" : "Review"}
              onClick={() => go("schedule-c")}
            />
            <Step
              n={2}
              icon={<UserCog size={18} />}
              done={profileDone}
              title="Set up your tax profile"
              detail={
                profileDone
                  ? `Filing status and last year's tax are saved, so estimates can use the safe-harbor rule.`
                  : "Add your filing status and last year's total tax so quarterly estimates have something solid to work from."
              }
              status={profileDone ? <span className="pill pill-pos">Done</span> : <span className="pill">Not set</span>}
              action={profileDone ? "Edit" : "Set up"}
              onClick={() => go("estimates")}
            />
            <Step
              n={3}
              icon={<CalendarClock size={18} />}
              done={!nextQuarter}
              title="Quarterly estimates"
              detail={
                nextQuarter ? (
                  <>
                    Next: <strong>Q{nextQuarter.quarter}</strong>, due {date(nextQuarter.due)} · plan about{" "}
                    <strong className="num">{money(nextQuarter.planned, { whole: true })}</strong>
                    {nextQuarter.paid_federal > 0 && <> ({money(nextQuarter.paid_federal, { whole: true })} paid so far)</>}
                  </>
                ) : (
                  "All four quarters are paid or have nothing due."
                )
              }
              status={
                nextQuarter ? (
                  <span className={`pill ${QUARTER_PILL[nextQuarter.status][1]}`}>{QUARTER_PILL[nextQuarter.status][0]}</span>
                ) : (
                  <span className="pill pill-pos">All set</span>
                )
              }
              action="Open"
              onClick={() => go("estimates")}
            />
            <Step
              n={4}
              icon={<Users size={18} />}
              done={needForm === 0 && maybeForm === 0}
              title="1099 contractors"
              detail={
                needForm === 0 && maybeForm === 0
                  ? `No one you paid by check or bank transfer reached the ${money(t99.data.threshold, { whole: true })} threshold.`
                  : [
                      needForm > 0 && `${plural(needForm, "contractor")} crossed ${money(t99.data.threshold, { whole: true })} and will need a 1099-NEC.`,
                      maybeForm > 0 && `${plural(maybeForm, "payee")} may need a 1099 — worth a check.`,
                    ]
                      .filter(Boolean)
                      .join(" ")
              }
              status={
                <span className="row" style={{ gap: 6 }}>
                  {needForm > 0 && <span className="pill pill-brand">{plural(needForm, "form")}</span>}
                  {maybeForm > 0 && <span className="pill pill-warn">{maybeForm} to check</span>}
                  {needForm === 0 && maybeForm === 0 && <span className="pill pill-pos">None needed</span>}
                </span>
              }
              action="Open"
              onClick={() => go("1099")}
            />
          </div>
        </Card>

        <Card title="What these estimates cover">
          <div className="stack" style={{ gap: 18 }}>
            <div>
              <div className="tx-list-head">Included</div>
              <ul className="tx-list">
                {[
                  "Schedule C profit from your categorized books",
                  "Self-employment tax (Schedule SE)",
                  "Federal income tax with the standard deduction",
                  "Qualified business income (QBI) deduction",
                  "Home office, simplified method only",
                ].map((x) => (
                  <li key={x}>
                    <Check size={14} className="pos" /> {x}
                  </li>
                ))}
              </ul>
            </div>
            <div>
              <div className="tx-list-head">Not included</div>
              <ul className="tx-list">
                {e.estimate.not_included.map((x) => (
                  <li key={x}>
                    <CircleSlash size={14} className="subtle" /> {x}
                  </li>
                ))}
              </ul>
            </div>
            <div className="tiny subtle">
              If any of the "not included" items apply to you, your real tax will differ — a tax pro can fold them in. Tax tables: {e.estimate.source}.
            </div>
          </div>
        </Card>
      </div>
    </div>
  );
}

function Step(props: {
  n: number;
  icon: ReactNode;
  done: boolean;
  title: string;
  detail: ReactNode;
  status: ReactNode;
  action: string;
  onClick: () => void;
}) {
  return (
    <div className="tx-step">
      <span className={`icon-bubble ${props.done ? "pos" : ""}`} role="img" aria-label={props.done ? "Done" : `Step ${props.n}`}>
        {props.done ? <Check size={18} strokeWidth={3} /> : props.icon}
      </span>
      <div className="grow">
        <div className="row" style={{ gap: 10, flexWrap: "wrap" }}>
          <span className="tx-step-title">{props.title}</span>
          {props.status}
        </div>
        <div className="tx-step-detail">{props.detail}</div>
      </div>
      <button className={`btn btn-sm ${props.done ? "btn-ghost" : "btn-secondary"}`} onClick={props.onClick}>
        {props.action} <ArrowRight size={14} />
      </button>
    </div>
  );
}
