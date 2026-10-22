// Due dates of household bills, each with what Tallybook knows about it and
// a way to mark it paid or skipped by hand.

import { Check, RotateCcw, SkipForward } from "lucide-react";
import { useAction } from "../api";
import { addDays, date, money, shortDate } from "../format";
import type { DueDate, DueState, ISODate } from "../types";
import { useAccountNames, useToast } from "./ui";
import "./DueList.css";

/** Each state's pill: its words and its color class ("" for plain). */
const STATE: Record<DueState, { label: string; tone: string }> = {
  paid: { label: "Paid", tone: "pos" },
  skipped: { label: "Skipped", tone: "" },
  overdue: { label: "Overdue", tone: "neg" },
  processing: { label: "Paying itself", tone: "brand" },
  due_today: { label: "Due today", tone: "warn" },
  upcoming: { label: "Coming up", tone: "" },
};

/** Still needs paying: neither paid nor skipped. */
const isOpen = (d: DueDate) => d.state !== "paid" && d.state !== "skipped";

export function DueRow({ due }: { due: DueDate }) {
  const toast = useToast();
  const accountName = useAccountNames();
  const mark = useAction<null, { bill_id: number; due_date: string; status: DueState | null }>("recurring.mark");
  const set = (status: DueState | null, text: string) =>
    mark
      .mutateAsync({ bill_id: due.bill_id, due_date: due.due_date, status })
      .then(() => toast({ text }))
      .catch((e) => toast.error(e));
  // The calendar-leaf date: "Mar" over "4".
  const [monthText, dayText] = shortDate(due.due_date).split(" ");
  const pill = STATE[due.state];
  const open = isOpen(due);
  // Anything set by hand can be undone, handing the date back to the bank feed.
  const manual = due.state === "skipped" || !!due.payment?.manual;
  return (
    <div className={`due-row due-${due.state}`}>
      <div className="due-date" title={date(due.due_date)}>
        <span className="due-date-m">{monthText}</span>
        <span className="due-date-d">{dayText}</span>
      </div>
      <div className="grow" style={{ minWidth: 0 }}>
        <div className="row" style={{ gap: 8, minWidth: 0 }}>
          <span className="bold truncate">{due.name}</span>
          {due.auto_pay && <span className="pill">Auto-pay</span>}
        </div>
        <div className="subtle small truncate">
          {accountName(due.account_id)}
          {due.payment && !due.payment.manual && ` · paid ${shortDate(due.payment.date)}`}
        </div>
      </div>
      <span className="num bold">{due.payment ? money(due.payment.amount) : due.amount ? money(due.amount) : "Varies"}</span>
      <span className={`pill ${pill.tone ? `pill-${pill.tone}` : ""}`}>{pill.label}</span>
      <div className="due-actions">
        {open && (
          <>
            <button className="btn btn-ghost btn-icon btn-sm" title="Mark paid" aria-label={`Mark ${due.name} paid`} onClick={() => set("paid", `${due.name} marked paid`)}>
              <Check size={15} />
            </button>
            <button className="btn btn-ghost btn-icon btn-sm" title="Skip this one" aria-label={`Skip ${due.name} this time`} onClick={() => set("skipped", `${due.name} skipped`)}>
              <SkipForward size={15} />
            </button>
          </>
        )}
        {manual && (
          <button className="btn btn-ghost btn-icon btn-sm" title="Undo" aria-label={`Undo for ${due.name}`} onClick={() => set(null, "Back to automatic")}>
            <RotateCcw size={15} />
          </button>
        )}
      </div>
    </div>
  );
}

/** A list of due dates, or `empty` (a sentence) when there are none. */
export function DueList({ items, empty }: { items: DueDate[]; empty?: string }) {
  if (!items.length) return <div className="muted small">{empty ?? "Nothing due."}</div>;
  return (
    <div className="due-list">
      {items.map((d) => (
        <DueRow key={`${d.bill_id}:${d.due_date}`} due={d} />
      ))}
    </div>
  );
}

/**
 * Splits due dates into the groups people think in: due now (today or
 * overdue), the next 7 days, later, and done (newest first). ISO dates
 * compare correctly as strings.
 */
export function groupDue(items: DueDate[], today: ISODate) {
  const weekEnd = addDays(today, 7);
  return {
    now: items.filter((d) => isOpen(d) && d.due_date <= today),
    week: items.filter((d) => isOpen(d) && d.due_date > today && d.due_date <= weekEnd),
    later: items.filter((d) => isOpen(d) && d.due_date > weekEnd),
    done: items.filter((d) => !isOpen(d)).reverse(),
  };
}
