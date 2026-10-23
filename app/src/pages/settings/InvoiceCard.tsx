// Invoice defaults: numbering, payment terms, default tax and footer.

import { FileText } from "lucide-react";
import { Field } from "../../components/ui";
import { SettingsCard, SaveFoot, useSavePrefs, type FormApi } from "./shared";

const INVOICE_KEYS = ["invoice.next_number", "invoice.default_terms_days", "invoice.default_tax_bp", "invoice.footer"];

const INVOICE_DEFAULTS: Record<string, string> = { "invoice.default_terms_days": "30" };

const TAX_TEXT = "__tax_pct";

function bpToPct(bp: string): string {
  const n = Number(bp);
  if (!bp || !Number.isFinite(n)) return "";
  return String(n / 100);
}

/** "8.25" → "825"; "" → ""; invalid → null. */
function pctToBp(pct: string): string | null {
  const s = pct.trim().replace(/%$/, "").trim();
  if (s === "") return "";
  if (!/^\d{0,3}(\.\d{0,3})?$/.test(s) || s === ".") return null;
  const n = Number(s);
  if (!Number.isFinite(n) || n > 100) return null;
  return String(Math.round(n * 100));
}

export default function InvoiceCard({ form }: { form: FormApi }) {
  const keys = [...INVOICE_KEYS, TAX_TEXT];
  const { run, busy } = useSavePrefs(form, keys, "Invoice settings");
  const taxText = form.get(TAX_TEXT, bpToPct(form.get("invoice.default_tax_bp")));
  const taxBp = pctToBp(taxText);
  const taxDirty = taxBp !== null && taxBp !== form.get("invoice.default_tax_bp");
  const dirty = form.dirty(INVOICE_KEYS, INVOICE_DEFAULTS) || taxDirty || taxBp === null;
  const nextNumber = form.get("invoice.next_number");

  return (
    <SettingsCard
      id="invoices"
      icon={<FileText size={18} />}
      title="Invoices"
      desc="Defaults for new invoices. You can still change any of these on a single invoice."
      foot={
        <SaveFoot
          dirty={dirty}
          busy={busy}
          onDiscard={() => form.reset(keys)}
          onSave={() => {
            if (taxBp === null) return;
            run({
              "invoice.next_number": nextNumber.trim() || null,
              "invoice.default_terms_days": form.get("invoice.default_terms_days", "30"),
              "invoice.default_tax_bp": taxBp || null,
              "invoice.footer": form.get("invoice.footer").trim() || null,
            });
          }}
        />
      }
    >
      <div className="form-grid">
        <Field label="Next invoice number" hint="The number your next invoice will get. Later ones count up from here.">
          <input className="input" value={nextNumber} onChange={(e) => form.set("invoice.next_number", e.target.value)} placeholder="1001" inputMode="numeric" />
        </Field>
        <Field label="Payment terms" hint="How long customers have to pay.">
          <select className="select" value={form.get("invoice.default_terms_days", "30")} onChange={(e) => form.set("invoice.default_terms_days", e.target.value)}>
            <option value="0">Due on receipt</option>
            <option value="15">Net 15 — due in 15 days</option>
            <option value="30">Net 30 — due in 30 days</option>
            <option value="60">Net 60 — due in 60 days</option>
          </select>
        </Field>
        <Field
          label="Default sales tax"
          hint={taxBp === null ? <span className="neg">Enter a percentage like 8.25</span> : "Applied to taxable lines. Leave blank if you don't charge sales tax."}
        >
          <div style={{ position: "relative" }}>
            <input className="input num" style={{ paddingRight: 32 }} value={taxText} onChange={(e) => form.set(TAX_TEXT, e.target.value)} placeholder="0" inputMode="decimal" />
            <span className="subtle bold" style={{ position: "absolute", right: 12, top: "50%", transform: "translateY(-50%)" }}>
              %
            </span>
          </div>
        </Field>
        <div />
        <Field label="Footer message" className="span-2" hint="Printed at the bottom of every invoice — a thank-you, payment instructions, etc.">
          <textarea className="textarea" rows={2} value={form.get("invoice.footer")} onChange={(e) => form.set("invoice.footer", e.target.value)} placeholder="Thank you for your business!" />
        </Field>
      </div>
    </SettingsCard>
  );
}
