// Due dates of household bills, each with what Tallybook knows about it and
// a way to mark it paid or skipped by hand.

import { Check, RotateCcw, SkipForward } from "lucide-react";
import { useAction } from "../api";
import { date, money, shortDate } from "../format";
import type { DueDate, DueState } from "../types";
import { useAccountNames, useToast } from "./ui";
import "./DueList.css";

const STATE: Record<DueState, { label: string; tone: string }> = {
  paid: { label: "Paid", tone: "pos" },
  skipped: { label: "Skipped", tone: "" },
  overdue: { label: "Overdue", tone: "neg" },
  processing: { label: "Paying itself", tone: "brand" },
  due_today: { label: "Due today", tone: "warn" },
  upcoming: { label: "Coming up", tone: "" },
};

export function DueRow({ due }: { due: DueDate }) {
  const toast = useToast();
  const name = useAccountNames();
  const mark = useAction<null, { bill_id: number; due_date: string; status: DueState | null }>("recurring.mark");
  const set = (status: DueState | null, text: string) =>
    mark
      .mutateAsync({ bill_id: due.bill_id, due_date: due.due_date, status })
      .then(() => toast({ text }))
      .catch((e) => toast.error(e));
  const [m, d] = shortDate(due.due_date).split(" ");
  const s = STATE[due.state];
  const open = due.state !== "paid" && due.state !== "skipped";
  const manual = due.state === "skipped" || !!due.payment?.manual;
  return (
    <div className={`due-row due-${due.state}`}>
      <div className="due-date" title={date(due.due_date)}>
        <span className="due-date-m">{m}</span>
        <span className="due-date-d">{d}</span>
      </div>
      <div className="grow" style={{ minWidth: 0 }}>
        <div className="row" style={{ gap: 8, minWidth: 0 }}>
          <span className="bold truncate">{due.name}</span>
          {due.auto_pay && <span className="pill">Auto-pay</span>}
        </div>
        <div className="subtle small truncate">
          {name(due.account_id)}
          {due.payment && !due.payment.manual && ` · paid ${shortDate(due.payment.date)}`}
        </div>
      </div>
      <span className="num bold">{due.payment ? money(due.payment.amount) : due.amount ? money(due.amount) : "Varies"}</span>
      <span className={`pill ${s.tone ? `pill-${s.tone}` : ""}`}>{s.label}</span>
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

/** Splits due dates into the groups people think in. */
export function groupDue(items: DueDate[], today: string) {
  const week = new Date(`${today}T00:00:00`);
  week.setDate(week.getDate() + 7);
  const weekEnd = `${week.getFullYear()}-${String(week.getMonth() + 1).padStart(2, "0")}-${String(week.getDate()).padStart(2, "0")}`;
  const open = (d: DueDate) => d.state !== "paid" && d.state !== "skipped";
  return {
    now: items.filter((d) => open(d) && d.due_date <= today),
    week: items.filter((d) => open(d) && d.due_date > today && d.due_date <= weekEnd),
    later: items.filter((d) => open(d) && d.due_date > weekEnd),
    done: items.filter((d) => !open(d)).reverse(),
  };
}
