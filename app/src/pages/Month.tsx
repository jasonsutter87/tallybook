// The household's month: what came in, what went out, and each budget line
// against the plan, judged by how far into the month it is.

import { ChevronLeft, ChevronRight, ClipboardList, Plus, Users } from "lucide-react";
import { useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { useApi } from "../api";
import { DueList, groupDue } from "../components/DueList";
import { Alerts, OpportunityCard, ScoreCard, WinsCard } from "../components/insights";
import { Meter, SENTIMENT_TONE, meterCaption, monthName, shiftMonth, thisMonth, zoneLabel } from "../components/household";
import { BigMoney, Card, EmptyState, ErrorBanner, Loading, Money, PageHeader } from "../components/ui";
import LogMoneyModal from "../flows/LogMoneyModal";
import PeopleModal from "../flows/PeopleModal";
import PlanModal from "../flows/PlanModal";
import { FLEX_LABEL, money, todayISO } from "../format";
import type { BudgetLineView, DueDate, MonthBudget } from "../types";
import "./Month.css";

export default function Month() {
  const params = useParams();
  const navigate = useNavigate();
  const month = params.month && /^\d{4}-\d{2}$/.test(params.month) ? params.month : thisMonth();
  const { data, error, isLoading } = useApi<MonthBudget>("budget.month", { month });
  const due = useApi<DueDate[]>("recurring.due");
  const [logging, setLogging] = useState(false);
  const [planning, setPlanning] = useState(false);
  const [peopleOpen, setPeopleOpen] = useState(false);

  const go = (m: string) => navigate(m === thisMonth() ? "/month" : `/month/${m}`);

  if (isLoading) return <Loading />;
  if (error || !data)
    return (
      <div className="page">
        <ErrorBanner error={error} />
      </div>
    );

  const net = data.income - data.spending;
  const left = data.available != null ? data.available - data.spending : null;
  const spendingLines = data.lines.filter((l) => l.direction === "ceiling");
  const goalLines = data.lines.filter((l) => l.direction === "floor");
  const maxCategory = Math.max(1, ...data.categories.map((c) => c.spending));
  const maxPerson = Math.max(1, ...data.people.map((p) => p.spending));
  const soon = (() => {
    const g = groupDue(due.data ?? [], todayISO());
    return [...g.now, ...g.week];
  })();

  return (
    <div className="page">
      <PageHeader
        title={
          <span className="row" style={{ gap: 6 }}>
            <button className="btn btn-ghost btn-icon" aria-label="Previous month" onClick={() => go(shiftMonth(month, -1))}>
              <ChevronLeft size={20} />
            </button>
            {monthName(month)}
            <button className="btn btn-ghost btn-icon" aria-label="Next month" onClick={() => go(shiftMonth(month, 1))}>
              <ChevronRight size={20} />
            </button>
          </span>
        }
        subtitle={
          <>
            {data.is_current ? `Day ${data.day_of_month} of ${data.days_in_month}` : data.day_of_month === data.days_in_month ? "Finished" : "Coming up"}
            {data.plan_from && ` · plan carried forward from ${monthName(data.plan_from, false)}`}
            {!data.is_current && (
              <>
                {" · "}
                <Link to="/month">Back to this month</Link>
              </>
            )}
          </>
        }
        actions={
          <>
            <button className="btn btn-secondary" onClick={() => setPlanning(true)}>
              <ClipboardList size={16} /> {data.lines.length ? "Edit plan" : "Make a plan"}
            </button>
            <button className="btn btn-primary" onClick={() => setLogging(true)}>
              <Plus size={16} /> Log money
            </button>
          </>
        }
      />

      <div className="stack" style={{ gap: 20 }}>
        {data.is_current && <Alerts month={month} />}
        <div className="hero month-hero">
          <div className="month-hero-main">
            <div className="hero-label">{left != null ? "Left to work with" : net >= 0 ? "More in than out" : "More out than in"}</div>
            <div className="hero-amount">
              <BigMoney cents={left ?? Math.abs(net)} />
            </div>
            {data.overall && data.available != null && (
              <div className="month-hero-meter">
                <Meter amount={data.available} progress={data.spending} status={data.overall} direction="ceiling" />
                <div className="month-hero-caption">{meterCaption(data.available, data.spending, data.overall, "ceiling")}</div>
              </div>
            )}
          </div>
          <div className="month-hero-figures">
            <Figure label="Came in" cents={data.income} />
            <Figure label="Went out" cents={data.spending} />
            <Figure label={net >= 0 ? "Ahead by" : "Behind by"} cents={Math.abs(net)} />
          </div>
        </div>

        {data.is_current && (due.data?.length ?? 0) > 0 && (
          <Card title="Due today & this week" action={<Link to="/due" className="small bold">All bills</Link>}>
            <DueList items={soon} empty="Nothing due this week." />
          </Card>
        )}

        {data.lines.length === 0 ? (
          <Card>
            <EmptyState
              icon={<ClipboardList size={22} />}
              title={`No plan for ${monthName(month, false)} yet`}
              body="Set a limit for the categories (or people) you want to keep an eye on. Next month starts with the same plan."
              action={
                <button className="btn btn-primary" onClick={() => setPlanning(true)}>
                  Make a plan
                </button>
              }
            />
          </Card>
        ) : (
          <div className={goalLines.length ? "month-grid" : undefined}>
            {spendingLines.length > 0 && (
              <Card title="Spending plan" action={<span className="subtle small">The notch is today</span>}>
                <div className="stack" style={{ gap: 16 }}>
                  {spendingLines.map((l) => (
                    <LineRow key={l.id} line={l} />
                  ))}
                </div>
              </Card>
            )}
            {goalLines.length > 0 && (
              <Card title="Saving & income goals">
                <div className="stack" style={{ gap: 16 }}>
                  {goalLines.map((l) => (
                    <LineRow key={l.id} line={l} />
                  ))}
                </div>
              </Card>
            )}
          </div>
        )}

        <div className="month-grid">
          <Card
            title="Who it was for"
            action={
              <button className="btn btn-ghost btn-sm" onClick={() => setPeopleOpen(true)}>
                <Users size={14} /> People
              </button>
            }
          >
            {data.people.length <= 1 ? (
              <div className="muted small">
                Add the people in your household to see what each of you costs.{" "}
                <button className="link-button" onClick={() => setPeopleOpen(true)}>
                  Add people
                </button>
              </div>
            ) : (
              <div className="stack" style={{ gap: 12 }}>
                {data.people.map((p) => (
                  <div key={p.person_id ?? "shared"}>
                    <div className="row-between" style={{ marginBottom: 6 }}>
                      <span className="bold truncate">{p.name}</span>
                      <Money cents={p.spending} className="bold" />
                    </div>
                    <div className="progress">
                      <div style={{ width: `${(Math.max(0, p.spending) / maxPerson) * 100}%` }} />
                    </div>
                  </div>
                ))}
              </div>
            )}
          </Card>

          <Card title="Where it went">
            {data.categories.length === 0 ? (
              <div className="muted small">Nothing spent yet this month.</div>
            ) : (
              <div className="stack" style={{ gap: 12 }}>
                {data.categories.slice(0, 8).map((c) => (
                  <Link key={c.account_id} to={`/accounts/${c.account_id}`} className="month-category">
                    <div className="row-between" style={{ marginBottom: 6 }}>
                      <span className="row" style={{ gap: 8, minWidth: 0 }}>
                        <span className="bold truncate">{c.name}</span>
                        {c.flex && <span className={`pill flex-${c.flex}`}>{FLEX_LABEL[c.flex]}</span>}
                      </span>
                      <Money cents={c.spending} className="bold" />
                    </div>
                    <div className="progress">
                      <div style={{ width: `${(Math.max(0, c.spending) / maxCategory) * 100}%` }} />
                    </div>
                  </Link>
                ))}
              </div>
            )}
          </Card>
        </div>
        <div className="month-grid">
          <div className="stack" style={{ gap: 16 }}>
            <WinsCard month={month} />
            <OpportunityCard month={month} />
          </div>
          <ScoreCard month={month} />
        </div>
      </div>

      <LogMoneyModal open={logging} onClose={() => setLogging(false)} scope="household" />
      <PlanModal open={planning} onClose={() => setPlanning(false)} month={data} />
      <PeopleModal open={peopleOpen} onClose={() => setPeopleOpen(false)} />
    </div>
  );
}

function Figure(props: { label: string; cents: number }) {
  return (
    <div>
      <div className="hero-label">{props.label}</div>
      <div className="num month-figure">{money(props.cents, { whole: true })}</div>
    </div>
  );
}

function LineRow({ line }: { line: BudgetLineView }) {
  const tone = SENTIMENT_TONE[line.status.sentiment];
  return (
    <div>
      <div className="row-between" style={{ marginBottom: 8, gap: 12 }}>
        <span className="row" style={{ gap: 8, minWidth: 0 }}>
          <span className="bold truncate">{line.name}</span>
          {line.target.kind === "person" && <span className="pill">Person</span>}
        </span>
        <span className={`pill pill-${tone}`}>{zoneLabel(line.direction, line.status.zone)}</span>
      </div>
      <Meter amount={line.amount} progress={line.progress} status={line.status} direction={line.direction} />
      <div className="subtle small" style={{ marginTop: 6 }}>
        {meterCaption(line.amount, line.progress, line.status, line.direction)}
      </div>
    </div>
  );
}
