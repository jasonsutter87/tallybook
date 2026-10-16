// Small helpers shared by the bill pages and the pay-bills flow.

import { useApi } from "../api";
import { daysBetween, plural, todayISO } from "../format";
import type { BillSummary, BookStatus, ISODate } from "../types";

/** The book's idea of "today" (falls back to the local clock). */
export function useToday(): ISODate {
  const { data } = useApi<BookStatus>("book.status");
  return data?.today ?? todayISO();
}

export function isUnpaid(b: BillSummary): boolean {
  return b.status !== "void" && b.balance > 0;
}

export function isOverdue(b: BillSummary, today: ISODate): boolean {
  return isUnpaid(b) && (b.status === "overdue" || b.due_date < today);
}

/** "Due today" / "Due in 3 days" / "5 days late" with a tone for coloring. */
export function dueHint(b: BillSummary, today: ISODate): { text: string; tone: "neg" | "warn" | "" } | null {
  if (!isUnpaid(b)) return null;
  const d = daysBetween(today, b.due_date);
  if (d < 0) return { text: `${plural(-d, "day")} late`, tone: "neg" };
  if (d === 0) return { text: "Due today", tone: "warn" };
  if (d <= 7) return { text: `Due in ${plural(d, "day")}`, tone: "warn" };
  return { text: `Due in ${plural(d, "day")}`, tone: "" };
}

export function DueHint({ bill, today }: { bill: BillSummary; today: ISODate }) {
  const h = dueHint(bill, today);
  if (!h) return null;
  const color = h.tone === "neg" ? "var(--neg)" : h.tone === "warn" ? "var(--warn)" : "var(--ink-3)";
  return (
    <div className="tiny" style={{ color, fontWeight: 700, marginTop: 2 }}>
      {h.text}
    </div>
  );
}

export const TERMS: { value: number; label: string }[] = [
  { value: 0, label: "Due on receipt" },
  { value: 15, label: "Net 15" },
  { value: 30, label: "Net 30" },
];

export const PAY_METHODS = ["ACH", "Check", "Card", "Cash", "Other"] as const;
