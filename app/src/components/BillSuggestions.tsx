// Repeat payments that look like bills: spotted in the books or straight from
// a bank import, offered as new bills with one click, or waved off for good.

import { ChevronRight, Plus, Sparkles, X } from "lucide-react";
import { Link } from "react-router-dom";
import { useAction, useApi } from "../api";
import { useSides } from "../book";
import type { BillDraft } from "../flows/RecurringBillModal";
import { money, plural, shortDate } from "../format";
import type { BillSuggestion, Cadence } from "../types";
import { Card, useAccountNames, useToast } from "./ui";

const nth = (n: number) => `${n}${n % 10 === 1 && n !== 11 ? "st" : n % 10 === 2 && n !== 12 ? "nd" : n % 10 === 3 && n !== 13 ? "rd" : "th"}`;

export function cadenceLabel(c: Cadence): string {
  if (c.every === "week") return "Weekly";
  if (c.every === "months") return c.months === 12 ? `Yearly on the ${nth(c.day)}` : `Every ${c.months} months on the ${nth(c.day)}`;
  return `Monthly on the ${nth(c.day)}`;
}

/** A new bill filled in from what was seen. A usage bill keeps its typical
 * amount, so only a payment near it ticks the bill off. */
export function suggestionDraft(s: BillSuggestion): BillDraft {
  return {
    name: s.name,
    amount: s.amount,
    varies: s.varies,
    account_id: s.account_id ?? undefined,
    paid_from_id: s.paid_from_id,
    person_id: s.person_id,
    cadence: s.cadence,
    start_date: s.start_date,
    match_text: s.match_text,
  };
}

function useSuggestions() {
  const sides = useSides();
  const q = useApi<BillSuggestion[]>("recurring.suggestions", undefined, { enabled: sides.household });
  return sides.household ? (q.data ?? []) : [];
}

/** Every suggestion, on the bills page. */
export function BillSuggestions(props: { onAdd: (draft: BillDraft) => void }) {
  const toast = useToast();
  const accountName = useAccountNames();
  const dismiss = useAction<null, { match_text: string }>("recurring.dismiss_suggestion");
  const suggestions = useSuggestions();
  if (!suggestions.length) return null;

  return (
    <Card title={<span className="row" style={{ gap: 8 }}><Sparkles size={16} /> These look like bills</span>}>
      <div className="stack-sm">
        {suggestions.map((s) => (
          <div key={s.match_text} className="row-between" style={{ gap: 12, flexWrap: "wrap" }}>
            <div style={{ minWidth: 0 }}>
              <div className="bold truncate">{s.name}</div>
              <div className="subtle small">
                {s.varies ? `About ${money(s.amount)}, varies` : money(s.amount)} · {cadenceLabel(s.cadence)}
                {s.account_id != null && ` · ${accountName(s.account_id)}`} · seen {s.times_seen} times, last {shortDate(s.last_seen)}
                {s.in_review > 0 && ` · ${s.in_review} in Review`}
              </div>
            </div>
            <div className="row" style={{ gap: 4 }}>
              <button
                className="btn btn-ghost btn-sm"
                disabled={dismiss.isPending}
                onClick={() =>
                  dismiss
                    .mutateAsync({ match_text: s.match_text })
                    .then(() => toast({ text: `Won't suggest ${s.name} again` }))
                    .catch((e) => toast.error(e))
                }
              >
                <X size={14} /> Not a bill
              </button>
              <button className="btn btn-secondary btn-sm" onClick={() => props.onAdd(suggestionDraft(s))}>
                <Plus size={14} /> Add as a bill
              </button>
            </div>
          </div>
        ))}
      </div>
    </Card>
  );
}

/** A nudge in Review when what was just imported repeats like a bill. */
export function ReviewBillNudge() {
  const fresh = useSuggestions().filter((s) => s.in_review > 0);
  if (!fresh.length) return null;
  const names = fresh.slice(0, 3).map((s) => s.name);
  const more = fresh.length - names.length;

  return (
    <Link to="/due" className="card card-pad row-between" style={{ gap: 12, marginBottom: 16, textDecoration: "none", color: "inherit" }}>
      <div className="row" style={{ gap: 10, minWidth: 0 }}>
        <Sparkles size={18} style={{ flexShrink: 0, color: "var(--brand)" }} />
        <div style={{ minWidth: 0 }}>
          <div className="bold">
            {plural(fresh.length, "repeat payment")} in here {fresh.length === 1 ? "looks like a bill" : "look like bills"}
          </div>
          <div className="subtle small truncate">
            {names.join(", ")}
            {more > 0 && ` and ${more} more`} · make them bills and they'll tick themselves off each time
          </div>
        </div>
      </div>
      <span className="row bold" style={{ gap: 4, flexShrink: 0 }}>
        Set them up <ChevronRight size={16} />
      </span>
    </Link>
  );
}
