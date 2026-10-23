// The Dashboard's money in/out bars. Its own module so the charting library
// loads only when this chart is on screen, not with every launch.

import { Bar, BarChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import { money, moneyCompact, monthLabel } from "../format";
import type { Dashboard } from "../types";

export default function CashFlowChart({ data }: { data: Dashboard["cashflow"] }) {
  // The bars as a sentence, for screen readers.
  const summary = data.map((m) => `${monthLabel(m.month)}: ${money(m.income)} in, ${money(m.expenses)} out`).join("; ");
  return (
    <div style={{ height: 250, marginLeft: -12 }} role="img" aria-label={`Cash flow by month. ${summary}`}>
      <ResponsiveContainer>
        <BarChart data={data} barGap={4} barCategoryGap="28%">
          <CartesianGrid vertical={false} stroke="var(--line)" />
          <XAxis dataKey="month" tickFormatter={monthLabel} tickLine={false} axisLine={false} tick={{ fill: "var(--ink-3)", fontSize: 12 }} />
          <YAxis tickFormatter={moneyCompact} tickLine={false} axisLine={false} width={56} tick={{ fill: "var(--ink-3)", fontSize: 12 }} />
          <Tooltip
            cursor={{ fill: "var(--surface-hover)" }}
            contentStyle={{ borderRadius: 12, border: "1px solid var(--line)", background: "var(--surface)", boxShadow: "var(--shadow)" }}
            labelFormatter={(m) => monthLabel(String(m))}
            formatter={(v, name) => [money(Number(v)), name === "income" ? "Money in" : "Money out"]}
          />
          <Bar dataKey="income" fill="var(--brand)" radius={[6, 6, 0, 0]} />
          <Bar dataKey="expenses" fill="var(--line-strong)" radius={[6, 6, 0, 0]} />
        </BarChart>
      </ResponsiveContainer>
    </div>
  );
}
