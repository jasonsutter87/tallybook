// The "Taxes" bit of a vendor: 1099 contractor, W-9 on file, legal name and
// the last four digits of their tax ID. Used by ContactModal and the 1099s
// tab. Tallybook never stores a full SSN/EIN.

import { useEffect, useState } from "react";
import { useAction } from "../api";
import { ErrorBanner, Field, Modal, useToast } from "../components/ui";
import type { Contact, ContactTaxInput } from "../types";

export function taxFromContact(c: Contact | null | undefined): ContactTaxInput {
  return {
    is_1099: c?.is_1099 ?? false,
    w9_on_file: c?.w9_on_file ?? false,
    legal_name: c?.legal_name ?? null,
    tin_last4: c?.tin_last4 ?? null,
  };
}

/** Error text for the last-4 box, or null when it's fine (blank is fine). */
export function tinError(v: string | null): string | null {
  const s = (v ?? "").trim();
  if (!s) return null;
  return /^\d{4}$/.test(s) ? null : "Enter exactly four digits.";
}

/** Blank strings become null so the backend stores "not set". */
export function cleanTax(t: ContactTaxInput): ContactTaxInput {
  const opt = (s: string | null) => (s && s.trim() ? s.trim() : null);
  return { ...t, legal_name: opt(t.legal_name), tin_last4: opt(t.tin_last4) };
}

export function ContactTaxFields(props: { value: ContactTaxInput; onChange: (v: ContactTaxInput) => void; showErrors?: boolean }) {
  const v = props.value;
  const set = <K extends keyof ContactTaxInput>(k: K, x: ContactTaxInput[K]) => props.onChange({ ...v, [k]: x });
  const err = tinError(v.tin_last4);
  return (
    <div className="stack" style={{ gap: 14 }}>
      <div className="stack-sm">
        <label className="switch" style={{ alignItems: "flex-start" }}>
          <input type="checkbox" style={{ flexShrink: 0, marginTop: 1 }} checked={v.is_1099} onChange={(e) => set("is_1099", e.target.checked)} />
          <span>
            1099 contractor
            <span className="subtle small" style={{ display: "block", fontWeight: 500 }}>
              A person or LLC (not a corporation) you pay for work. Tallybook totals what you pay them each year.
            </span>
          </span>
        </label>
        <label className="switch" style={{ alignItems: "flex-start" }}>
          <input type="checkbox" style={{ flexShrink: 0, marginTop: 1 }} checked={v.w9_on_file} onChange={(e) => set("w9_on_file", e.target.checked)} />
          <span>
            W-9 on file
            <span className="subtle small" style={{ display: "block", fontWeight: 500 }}>
              The IRS form where they give you their legal name and tax ID. Ask for it before you pay them.
            </span>
          </span>
        </label>
      </div>
      <div className="form-grid">
        <Field label="Legal name (from W-9)" hint="Leave blank if it's the same as the contact name">
          <input className="input" value={v.legal_name ?? ""} onChange={(e) => set("legal_name", e.target.value)} placeholder="e.g. Maya L. Chen" />
        </Field>
        <Field
          label="Tax ID — last 4 digits only"
          hint={
            err && props.showErrors !== false ? (
              <span className="neg">{err}</span>
            ) : (
              "Tallybook keeps only the last four digits. The full number stays on the W-9."
            )
          }
        >
          <input
            className="input num"
            inputMode="numeric"
            maxLength={4}
            placeholder="1234"
            value={v.tin_last4 ?? ""}
            style={err ? { borderColor: "var(--neg)", textAlign: "left" } : { textAlign: "left" }}
            onChange={(e) => set("tin_last4", e.target.value.replace(/[^\d]/g, "").slice(0, 4))}
          />
        </Field>
      </div>
    </div>
  );
}

/** Small modal to edit just a contact's tax details. */
export function ContactTaxModal(props: { open: boolean; onClose: () => void; contactId: number | null; name: string; initial: ContactTaxInput }) {
  const toast = useToast();
  const save = useAction<null, { id: number } & ContactTaxInput>("tax.set_contact");
  const [form, setForm] = useState<ContactTaxInput>(props.initial);
  const [error, setError] = useState<Error | null>(null);

  useEffect(() => {
    if (props.open) {
      setForm(props.initial);
      setError(null);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.open, props.contactId]);

  const submit = async () => {
    if (props.contactId == null) return;
    const e = tinError(form.tin_last4);
    if (e) {
      setError(new Error(e));
      return;
    }
    try {
      await save.mutateAsync({ id: props.contactId, ...cleanTax(form) });
      toast({ text: `Tax details saved for ${props.name}` });
      props.onClose();
    } catch (err) {
      setError(err as Error);
    }
  };

  return (
    <Modal
      open={props.open}
      onClose={props.onClose}
      width={560}
      title={`Tax details · ${props.name}`}
      subtitle="Used to prepare your 1099 summary. Nothing is sent anywhere."
      footer={
        <>
          <button className="btn btn-secondary" onClick={props.onClose}>
            Cancel
          </button>
          <button className="btn btn-primary" onClick={submit} disabled={save.isPending}>
            {save.isPending ? "Saving…" : "Save"}
          </button>
        </>
      }
    >
      <div className="stack">
        <ErrorBanner error={error} />
        <ContactTaxFields value={form} onChange={setForm} />
      </div>
    </Modal>
  );
}
