// Household building blocks: the budget meter, status words, and the person picker.

import { useMemo } from "react";
import { useApi } from "../api";
import { money } from "../format";
import type { BudgetDirection, BudgetStatus, Person, Sentiment, Zone } from "../types";
import { Combobox, type ComboOption } from "./ui";

/** Status in plain words, phrased for which way the line is meant to go. */
export function zoneLabel(direction: BudgetDirection, zone: Zone): string {
  if (direction === "floor") {
    return { under: "Behind", in_range: "On track", trending_over: "Ahead", over: "Done" }[zone];
  }
  return { under: "Under", in_range: "On plan", trending_over: "Running hot", over: "Over" }[zone];
}

export const SENTIMENT_TONE: Record<Sentiment, string> = { good: "pos", caution: "warn", alarm: "neg" };

/**
 * The bar is the budget. The fill is what's happened. The notch is today.
 * If the fill is about level with the notch, the month is on plan; nobody
 * has to do arithmetic.
 */
export function Meter(props: { amount: number; progress: number; status: BudgetStatus; direction: BudgetDirection; showNotch?: boolean }) {
  const { amount, progress, status } = props;
  const fill = amount > 0 ? Math.min(1, Math.max(0, progress) / amount) : progress > 0 ? 1 : 0;
  const tone = SENTIMENT_TONE[status.sentiment];
  return (
    <div
      className={`meter meter-${tone}`}
      role="meter"
      aria-valuemin={0}
      aria-valuemax={amount}
      aria-valuenow={Math.max(0, progress)}
      aria-label={`${money(Math.max(0, progress))} of ${money(amount)}, ${zoneLabel(props.direction, status.zone).toLowerCase()}`}
    >
      <div className="meter-fill" style={{ width: `${fill * 100}%` }} />
      {props.showNotch !== false && status.elapsed < 1 && <div className="meter-notch" style={{ left: `${status.elapsed * 100}%` }} />}
    </div>
  );
}

/** "$148 of $400 · on pace for $493", or "$180 left". */
export function meterCaption(amount: number, progress: number, status: BudgetStatus, direction: BudgetDirection): string {
  const head = `${money(Math.max(0, progress), { whole: true })} of ${money(amount, { whole: true })}`;
  if (status.elapsed >= 1) {
    const diff = amount - progress;
    if (direction === "ceiling") return diff >= 0 ? `${head} · ${money(diff, { whole: true })} under` : `${head} · ${money(-diff, { whole: true })} over`;
    return diff > 0 ? `${head} · ${money(diff, { whole: true })} short` : `${head} · reached`;
  }
  if (direction === "ceiling") {
    if (status.remaining < 0) return `${head} · ${money(-status.remaining, { whole: true })} over`;
    return `${head} · on pace for ${money(status.pace, { whole: true })}`;
  }
  return status.remaining > 0 ? `${head} · ${money(status.remaining, { whole: true })} to go` : `${head} · reached`;
}

export function usePeople(includeArchived = false) {
  const q = useApi<Person[]>("people.list");
  const people = useMemo(() => (q.data ?? []).filter((p) => includeArchived || !p.archived), [q.data, includeArchived]);
  return { ...q, people };
}

/** Who the money was for. Empty means shared. */
export function PersonSelect(props: { value: number | null; onChange: (id: number | null) => void; placeholder?: string }) {
  const { people } = usePeople();
  const options = useMemo<ComboOption[]>(() => people.map((p) => ({ value: p.id, label: p.name })), [people]);
  if (!people.length) return null;
  return (
    <Combobox
      options={[{ value: 0, label: "Shared (everyone)" }, ...options]}
      value={props.value ?? 0}
      onChange={(v) => props.onChange(v ? v : null)}
      placeholder={props.placeholder ?? "Who was it for?"}
    />
  );
}

/** Month arithmetic on "YYYY-MM". */
export function shiftMonth(month: string, by: number): string {
  const [y, m] = month.split("-").map(Number);
  const d = new Date(y, m - 1 + by, 1);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}`;
}

export function monthName(month: string, withYear = true): string {
  const [y, m] = month.split("-").map(Number);
  return new Date(y, m - 1, 1).toLocaleDateString("en-US", withYear ? { month: "long", year: "numeric" } : { month: "long" });
}

export function thisMonth(): string {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}`;
}
