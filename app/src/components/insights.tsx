// The household's month, summed up: alerts, the budget score, where the room
// to save is, and wins worth saying out loud.

import { AlertCircle, AlertTriangle, Info, PartyPopper, Sparkles } from "lucide-react";
import { useState } from "react";
import { useApi } from "../api";
import { FLEX_LABEL, money } from "../format";
import type { BudgetScore, HouseholdAlert, Opportunities, ScoreBand, Win } from "../types";
import { Card, Modal } from "./ui";
import "./insights.css";

const ALERT_ICON = { alarm: AlertCircle, caution: AlertTriangle, info: Info };

/** Alerts past this many collapse into "And N more", so the month stays readable. */
const ALERTS_SHOWN = 4;
/** "Biggest that can bend" lists this many categories. */
const MOVABLE_SHOWN = 4;

/** What needs attention this month, most urgent first (the backend sorts them). */
export function Alerts({ month }: { month: string }) {
  const { data } = useApi<HouseholdAlert[]>("household.alerts", { month });
  if (!data?.length) return null;
  return (
    <div className="hh-alerts" role="status">
      {data.slice(0, ALERTS_SHOWN).map((a, i) => {
        const Icon = ALERT_ICON[a.tone];
        return (
          <div key={i} className={`hh-alert hh-alert-${a.tone}`}>
            <Icon size={17} />
            <div>
              <div className="bold">{a.title}</div>
              <div className="small">{a.body}</div>
            </div>
          </div>
        );
      })}
      {data.length > ALERTS_SHOWN && <div className="subtle small">And {data.length - ALERTS_SHOWN} more.</div>}
    </div>
  );
}

// The score runs 300..850, like a US credit score. The band cut-offs below
// must match band_for() in crates/tally-core/src/insights.rs.
const SCORE_MIN = 300;
const SCORE_MAX = 850;

const BAND_LABEL: Record<ScoreBand, string> = { poor: "Poor", fair: "Fair", good: "Good", very_good: "Very good", excellent: "Excellent" };
const BAND_TONE: Record<ScoreBand, string> = { poor: "neg", fair: "warn", good: "brand", very_good: "pos", excellent: "pos" };
const BANDS: [ScoreBand, string][] = [
  ["poor", "300–579"],
  ["fair", "580–669"],
  ["good", "670–739"],
  ["very_good", "740–799"],
  ["excellent", "800–850"],
];

/**
 * Shaped like a credit score so the number needs no explaining. It is not
 * one, and the asterisk says so plainly.
 */
export function ScoreCard({ month }: { month: string }) {
  const { data } = useApi<BudgetScore>("household.score", { month });
  const [about, setAbout] = useState(false);
  if (!data) return null;
  // Where the dot sits on the scale, as a percent from the left.
  const position = ((data.score - SCORE_MIN) / (SCORE_MAX - SCORE_MIN)) * 100;
  return (
    <Card
      title={
        <span>
          Budget score
          <button className="hh-asterisk" aria-label="What is the budget score?" onClick={() => setAbout(true)}>
            *
          </button>
        </span>
      }
    >
      <div className="row" style={{ gap: 14, alignItems: "baseline" }}>
        <div className="hh-score num">{data.score}</div>
        <span className={`pill pill-${BAND_TONE[data.band]}`}>{BAND_LABEL[data.band]}</span>
      </div>
      <div className="hh-score-scale" aria-hidden="true">
        <div className="hh-score-dot" style={{ left: `${position}%` }} />
      </div>
      <div className="stack-sm" style={{ marginTop: 14 }}>
        {data.factors.map((f) => (
          <div key={f.key} className="hh-factor">
            <div className="row-between">
              <span className="bold small">{f.label}</span>
              <span className="subtle small num">
                {Math.round(f.points)} / {f.weight}
              </span>
            </div>
            <div className="progress" style={{ height: 6 }}>
              <div style={{ width: `${f.fraction * 100}%` }} />
            </div>
            <div className="subtle small">{f.summary}</div>
          </div>
        ))}
      </div>
      <Modal
        open={about}
        onClose={() => setAbout(false)}
        title="About the budget score"
        footer={
          <button className="btn btn-primary" onClick={() => setAbout(false)}>
            Got it
          </button>
        }
      >
        <div className="stack">
          <div className="banner">
            <b>This is not a credit score.</b> It has nothing to do with credit, isn't reported to anyone, isn't shared with any company, and changes
            nothing about what you can borrow. It never leaves your book. It borrows a credit score's shape only so the number needs no explaining.
          </div>
          <p className="muted">It adds up five things about your household's money, weighted by how much they matter:</p>
          <div className="stack-sm">
            {data.factors.map((f) => (
              <div key={f.key} className="row-between">
                <span>{f.label}</span>
                <span className="subtle num">{f.weight} points</span>
              </div>
            ))}
          </div>
          <div className="row" style={{ gap: 6, flexWrap: "wrap" }}>
            {BANDS.map(([b, range]) => (
              <span key={b} className={`pill ${b === data.band ? `pill-${BAND_TONE[b]}` : ""}`}>
                {BAND_LABEL[b]} {range}
              </span>
            ))}
          </div>
          <p className="muted small">
            What it can't see: money that never reaches your book, and bills paid in ways the bank feed doesn't show (mark those paid by hand).
            Living within your income looks back six months, so one late check doesn't swing it, and steady income only counts for a little,
            because freelance money arrives when it arrives.
          </p>
        </div>
      </Modal>
    </Card>
  );
}

/** Where the give is: spending split by how far it can bend. */
export function OpportunityCard({ month }: { month: string }) {
  const { data } = useApi<Opportunities>("household.opportunities", { month });
  if (!data || data.spending <= 0) return null;
  // Sum of the tiers, for sizing the stacked bar; at least 1 to never divide by zero.
  const total = Math.max(1, data.tiers.reduce((n, t) => n + Math.max(0, t.spending), 0));
  const short = data.gap < 0;
  // Anything not fixed can bend, including categories nobody has sorted yet.
  const movable = data.categories.filter((c) => c.flex !== "fixed").slice(0, MOVABLE_SHOWN);
  return (
    <Card title="Where there's room">
      <p className="small" style={{ marginBottom: 12 }}>
        {short ? (
          <>
            <b>{money(-data.gap, { whole: true })} more went out than came in.</b> Trimming your-choice spending by a fifth would free up{" "}
            {money(data.lifestyle_fifth, { whole: true })}.
          </>
        ) : (
          <>
            <b>{money(data.gap, { whole: true })} more came in than went out.</b> Your-choice spending is where any extra room comes from.
          </>
        )}
      </p>
      <div className="hh-tiers" role="img" aria-label="Spending split by how far it can bend">
        {data.tiers
          .filter((t) => t.spending > 0)
          .map((t) => (
            <div key={t.flex ?? "unsorted"} className={`hh-tier flex-${t.flex ?? "none"}`} style={{ flexGrow: t.spending / total }} />
          ))}
      </div>
      <div className="stack-sm" style={{ marginTop: 12 }}>
        {data.tiers
          .filter((t) => t.spending > 0 || t.usual > 0)
          .map((t) => (
            <div key={t.flex ?? "unsorted"} className="row-between small">
              <span className="row" style={{ gap: 8 }}>
                <span className={`hh-swatch flex-${t.flex ?? "none"}`} />
                {t.flex ? FLEX_LABEL[t.flex] : "Not sorted yet"}
              </span>
              <span className="num">
                <b>{money(t.spending, { whole: true })}</b> <span className="subtle">usually {money(t.usual, { whole: true })}</span>
              </span>
            </div>
          ))}
      </div>
      {movable.length > 0 && (
        <>
          <div className="label" style={{ marginTop: 16 }}>
            Biggest that can bend
          </div>
          <div className="stack-sm">
            {movable.map((c) => (
              <div key={c.account_id} className="row-between small">
                <span>{c.name}</span>
                <span className="num">
                  <b>{money(c.spending, { whole: true })}</b>
                  {c.usual > 0 && c.spending > c.usual && <span className="neg"> +{money(c.spending - c.usual, { whole: true })}</span>}
                </span>
              </div>
            ))}
          </div>
        </>
      )}
    </Card>
  );
}

/** Good news from the month, said out loud. Hidden when there's none. */
export function WinsCard({ month }: { month: string }) {
  const { data } = useApi<Win[]>("household.wins", { month });
  if (!data?.length) return null;
  return (
    <Card title={<span className="row" style={{ gap: 8 }}><PartyPopper size={16} /> Nice work</span>}>
      <div className="stack-sm">
        {data.map((w, i) => (
          <div key={i} className="hh-win">
            <Sparkles size={15} />
            <div>
              <div className="bold">{w.title}</div>
              <div className="subtle small">{w.body}</div>
            </div>
          </div>
        ))}
      </div>
    </Card>
  );
}
