// Minimal "new customer / new vendor" form used inline from editors.
// The full contact editor lives on the Contacts page.

import { useState } from "react";
import { useAction } from "../api";
import { Field, Modal, useToast } from "../components/ui";
import type { NewContact } from "../types";

export default function QuickContactModal(props: {
  open: boolean;
  onClose: () => void;
  kind: "customer" | "vendor";
  onCreated: (id: number) => void;
}) {
  if (!props.open) return null;
  return <QuickContactForm {...props} />;
}

function QuickContactForm(props: { onClose: () => void; kind: "customer" | "vendor"; onCreated: (id: number) => void }) {
  const toast = useToast();
  const create = useAction<number, NewContact>("contacts.create");
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [company, setCompany] = useState("");
  const [error, setError] = useState<string | null>(null);
  const noun = props.kind === "customer" ? "customer" : "vendor";

  const submit = async () => {
    if (!name.trim()) {
      setError(`Give your ${noun} a name.`);
      return;
    }
    setError(null);
    try {
      const id = await create.mutateAsync({
        kind: props.kind,
        name: name.trim(),
        company: company.trim() || null,
        email: email.trim() || null,
        phone: null,
        address: null,
        notes: null,
      });
      toast({ text: `${name.trim()} added` });
      props.onCreated(id);
      props.onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <Modal
      open
      onClose={props.onClose}
      title={`New ${noun}`}
      subtitle="Just the basics. You can add an address and notes later on the Contacts page."
      width={460}
      footer={
        <>
          <button className="btn btn-secondary" onClick={props.onClose}>
            Cancel
          </button>
          <button className="btn btn-primary" onClick={submit} disabled={create.isPending}>
            {create.isPending ? "Saving…" : `Add ${noun}`}
          </button>
        </>
      }
    >
      <form
        className="stack"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        {error && <div className="banner error">{error}</div>}
        <Field label="Name">
          <input className="input" autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder={props.kind === "customer" ? "e.g. Bluebird Coffee" : "e.g. Adobe"} />
        </Field>
        <Field label="Email" hint={props.kind === "customer" ? "Used when you email an invoice." : undefined}>
          <input className="input" type="email" value={email} onChange={(e) => setEmail(e.target.value)} placeholder="name@example.com" />
        </Field>
        <Field label="Company" hint="Optional">
          <input className="input" value={company} onChange={(e) => setCompany(e.target.value)} />
        </Field>
        <button type="submit" hidden />
      </form>
    </Modal>
  );
}
