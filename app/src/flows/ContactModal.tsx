// Create or edit a customer / vendor. Used by Contacts, the bill editor
// ("New vendor") and anywhere else that needs a full contact form.

import { useEffect, useState, type FormEvent } from "react";
import { useAction } from "../api";
import { ErrorBanner, Field, Modal, useToast } from "../components/ui";
import type { Contact, ContactKind, ContactTaxInput, NewContact } from "../types";
import { ContactTaxFields, cleanTax, taxFromContact, tinError } from "./ContactTaxFields";

const KINDS: { value: ContactKind; label: string; hint: string }[] = [
  { value: "customer", label: "Customer", hint: "Pays you" },
  { value: "vendor", label: "Vendor", hint: "You pay them" },
  { value: "both", label: "Both", hint: "Either way" },
];

function blank(kind: ContactKind): NewContact {
  return { kind, name: "", company: null, email: null, phone: null, address: null, notes: null };
}

function fromContact(c: Contact): NewContact {
  return { kind: c.kind, name: c.name, company: c.company, email: c.email, phone: c.phone, address: c.address, notes: c.notes };
}

/** Empty strings become null so the backend stores "not set". */
function clean(c: NewContact): NewContact {
  const opt = (s: string | null) => (s && s.trim() ? s.trim() : null);
  return {
    kind: c.kind,
    name: c.name.trim(),
    company: opt(c.company),
    email: opt(c.email),
    phone: opt(c.phone),
    address: opt(c.address),
    notes: opt(c.notes),
  };
}

export default function ContactModal(props: {
  open: boolean;
  onClose: () => void;
  contact?: Contact;
  defaultKind?: ContactKind;
  onSaved?: (id: number) => void;
}) {
  const toast = useToast();
  const create = useAction<number, NewContact>("contacts.create");
  const update = useAction<null, { id: number; contact: NewContact }>("contacts.update");
  const [form, setForm] = useState<NewContact>(() => (props.contact ? fromContact(props.contact) : blank(props.defaultKind ?? "customer")));
  const [tax, setTax] = useState<ContactTaxInput>(() => taxFromContact(props.contact));
  const saveTax = useAction<null, { id: number } & ContactTaxInput>("tax.set_contact");
  const [error, setError] = useState<Error | null>(null);

  // Reset whenever the modal is (re)opened for a different contact.
  useEffect(() => {
    if (!props.open) return;
    setForm(props.contact ? fromContact(props.contact) : blank(props.defaultKind ?? "customer"));
    setTax(taxFromContact(props.contact));
    setError(null);
  }, [props.open, props.contact, props.defaultKind]);

  const set = <K extends keyof NewContact>(k: K, v: NewContact[K]) => setForm((f) => ({ ...f, [k]: v }));
  const busy = create.isPending || update.isPending || saveTax.isPending;
  const paysThem = form.kind === "vendor" || form.kind === "both";
  const editing = !!props.contact;

  const submit = async (e?: FormEvent) => {
    e?.preventDefault();
    const c = clean(form);
    if (!c.name) {
      setError(new Error("Give this contact a name."));
      return;
    }
    const tinErr = paysThem ? tinError(tax.tin_last4) : null;
    if (tinErr) {
      setError(new Error(`Tax ID: ${tinErr}`));
      return;
    }
    try {
      let id: number;
      if (props.contact) {
        await update.mutateAsync({ id: props.contact.id, contact: c });
        id = props.contact.id;
      } else {
        id = await create.mutateAsync(c);
      }
      // Tax details only apply to people you pay; skip the call when nothing would change.
      const before = taxFromContact(props.contact);
      const after = cleanTax(tax);
      const changed = (Object.keys(after) as (keyof ContactTaxInput)[]).some((k) => after[k] !== before[k]);
      if (paysThem && changed) await saveTax.mutateAsync({ id, ...after });
      toast({ text: editing ? `${c.name} updated` : `${c.name} added` });
      props.onSaved?.(id);
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
      title={editing ? `Edit ${props.contact!.name}` : "Add a contact"}
      subtitle={editing ? undefined : "Someone you sell to, buy from, or both."}
      footer={
        <>
          <button className="btn btn-secondary" onClick={props.onClose} type="button">
            Cancel
          </button>
          <button className="btn btn-primary" onClick={() => submit()} disabled={busy} type="button">
            {busy ? "Saving…" : editing ? "Save changes" : "Add contact"}
          </button>
        </>
      }
    >
      <form onSubmit={submit} className="stack" style={{ gap: 16 }}>
        <ErrorBanner error={error} />
        <div>
          <div className="label" style={{ marginBottom: 6 }}>
            This is a…
          </div>
          <div className="grid-3" style={{ gap: 10 }}>
            {KINDS.map((k) => {
              const on = form.kind === k.value;
              return (
                <button
                  key={k.value}
                  type="button"
                  onClick={() => set("kind", k.value)}
                  aria-pressed={on}
                  style={{
                    textAlign: "left",
                    padding: "10px 14px",
                    borderRadius: 14,
                    cursor: "pointer",
                    border: `1.5px solid ${on ? "var(--brand)" : "var(--line-strong)"}`,
                    background: on ? "var(--brand-soft)" : "var(--surface)",
                    color: on ? "var(--brand-soft-ink)" : "var(--ink)",
                  }}
                >
                  <div className="bold">{k.label}</div>
                  <div className="small" style={{ color: on ? "var(--brand-soft-ink)" : "var(--ink-3)" }}>
                    {k.hint}
                  </div>
                </button>
              );
            })}
          </div>
        </div>
        <div className="form-grid">
          <Field label="Name">
            <input className="input" autoFocus value={form.name} onChange={(e) => set("name", e.target.value)} placeholder="e.g. Bluebird Coffee" />
          </Field>
          <Field label="Company" hint="Optional">
            <input className="input" value={form.company ?? ""} onChange={(e) => set("company", e.target.value)} />
          </Field>
          <Field label="Email">
            <input className="input" type="email" value={form.email ?? ""} onChange={(e) => set("email", e.target.value)} placeholder="billing@example.com" />
          </Field>
          <Field label="Phone">
            <input className="input" type="tel" value={form.phone ?? ""} onChange={(e) => set("phone", e.target.value)} />
          </Field>
          <Field label="Address" className="span-2">
            <textarea className="textarea" rows={3} value={form.address ?? ""} onChange={(e) => set("address", e.target.value)} placeholder={"Street\nCity, State ZIP"} />
          </Field>
          <Field label="Notes" className="span-2" hint="Only you see these">
            <textarea className="textarea" rows={2} value={form.notes ?? ""} onChange={(e) => set("notes", e.target.value)} />
          </Field>
        </div>
        {paysThem && (
          <div className="stack-sm" style={{ borderTop: "1px solid var(--line)", paddingTop: 16 }}>
            <div>
              <div className="bold">Taxes</div>
              <div className="subtle small">If you pay them for work, this helps Tallybook prepare your 1099 summary.</div>
            </div>
            <ContactTaxFields value={tax} onChange={setTax} />
          </div>
        )}
        <button type="submit" hidden />
      </form>
    </Modal>
  );
}
