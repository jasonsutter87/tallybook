// Business profile card: name, email, phone and address, which print on invoices.

import { Building2 } from "lucide-react";
import { Field } from "../../components/ui";
import { SettingsCard, SaveFoot, useSavePrefs, type FormApi } from "./shared";

const BUSINESS_KEYS = ["company.name", "company.email", "company.phone", "company.address"];

export default function BusinessCard({ form }: { form: FormApi }) {
  const { run, busy } = useSavePrefs(form, BUSINESS_KEYS, "Business profile");
  const dirty = form.dirty(BUSINESS_KEYS);
  const nameMissing = form.get("company.name").trim() === "";
  return (
    <SettingsCard
      id="business"
      icon={<Building2 size={18} />}
      title="Business profile"
      desc="Shown on your invoices and at the top of printed reports."
      foot={
        <SaveFoot
          dirty={dirty}
          busy={busy}
          onDiscard={() => form.reset(BUSINESS_KEYS)}
          onSave={() => {
            if (nameMissing) return;
            run(Object.fromEntries(BUSINESS_KEYS.map((k) => [k, form.get(k).trim() || null])));
          }}
        />
      }
    >
      <div className="form-grid">
        <Field label="Business name" className="span-2" hint={nameMissing ? <span className="neg">Your business needs a name.</span> : undefined}>
          <input className="input" value={form.get("company.name")} onChange={(e) => form.set("company.name", e.target.value)} placeholder="Northwind Studio" />
        </Field>
        <Field label="Email">
          <input className="input" type="email" value={form.get("company.email")} onChange={(e) => form.set("company.email", e.target.value)} placeholder="hello@yourbusiness.com" />
        </Field>
        <Field label="Phone">
          <input className="input" type="tel" value={form.get("company.phone")} onChange={(e) => form.set("company.phone", e.target.value)} placeholder="(555) 123-4567" />
        </Field>
        <Field label="Address" className="span-2">
          <textarea className="textarea" rows={3} value={form.get("company.address")} onChange={(e) => form.set("company.address", e.target.value)} placeholder={"123 Main St\nCity, ST 00000"} />
        </Field>
      </div>
    </SettingsCard>
  );
}
