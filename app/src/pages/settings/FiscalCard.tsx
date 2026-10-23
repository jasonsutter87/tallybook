// Fiscal year card: the month the year starts.

import { CalendarRange } from "lucide-react";
import { useEffect, useState } from "react";
import { useAction } from "../../api";
import { Field, useToast } from "../../components/ui";
import { SettingsCard, SaveFoot } from "./shared";

const MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

export default function FiscalCard({ current }: { current: number }) {
  const toast = useToast();
  const save = useAction<void, unknown>("settings.set");
  const [month, setMonth] = useState(current);
  useEffect(() => setMonth(current), [current]);
  const dirty = month !== current;
  return (
    <SettingsCard
      id="fiscal"
      icon={<CalendarRange size={18} />}
      title="Fiscal year"
      desc="When your business year starts. Most small businesses use January."
      foot={
        <SaveFoot
          dirty={dirty}
          busy={save.isPending}
          onDiscard={() => setMonth(current)}
          onSave={async () => {
            try {
              await save.mutateAsync({ fiscal_year_start_month: month });
              toast({ text: "Fiscal year saved" });
            } catch (e) {
              toast.error(e);
            }
          }}
        />
      }
    >
      <div className="form-grid">
        <Field label="First month of the year" hint={`"Year to date" reports and "Net income (this year)" on the balance sheet count from the 1st of ${MONTHS[month - 1]}.`}>
          <select className="select" value={month} onChange={(e) => setMonth(Number(e.target.value))}>
            {MONTHS.map((m, i) => (
              <option key={m} value={i + 1}>
                {m}
              </option>
            ))}
          </select>
        </Field>
      </div>
    </SettingsCard>
  );
}
