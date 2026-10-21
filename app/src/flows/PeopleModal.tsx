// The people a household's money is for. Includes you.

import { Archive, ArchiveRestore } from "lucide-react";
import { useState } from "react";
import { useAction } from "../api";
import { usePeople } from "../components/household";
import { ErrorBanner, Modal, useToast } from "../components/ui";

export default function PeopleModal(props: { open: boolean; onClose: () => void }) {
  const toast = useToast();
  const { people } = usePeople(true);
  const create = useAction<number>("people.create");
  const update = useAction("people.update");
  const archive = useAction("people.archive");
  const [name, setName] = useState("");

  const add = async () => {
    if (!name.trim()) return;
    try {
      await create.mutateAsync({ name: name.trim() });
      setName("");
    } catch {
      /* shown inline */
    }
  };

  return (
    <Modal
      open={props.open}
      onClose={props.onClose}
      title="Who the money is for"
      subtitle="Tag spending with a person to see, and budget, what each of you costs. Untagged spending counts as shared."
      footer={
        <button className="btn btn-primary" onClick={props.onClose}>
          Done
        </button>
      }
    >
      <div className="stack">
        <ErrorBanner error={create.error ?? update.error ?? archive.error} />
        <div className="stack-sm">
          {people.map((p) => (
            <div key={p.id} className={`row ${p.archived ? "muted" : ""}`} style={{ gap: 8 }}>
              <input
                className="input grow"
                defaultValue={p.name}
                aria-label={`Name for ${p.name}`}
                onBlur={(e) => {
                  const v = e.target.value.trim();
                  if (v && v !== p.name) update.mutateAsync({ id: p.id, person: { name: v } }).catch((err) => toast.error(err));
                }}
              />
              <button
                className="btn btn-ghost btn-icon"
                aria-label={p.archived ? `Bring back ${p.name}` : `Archive ${p.name}`}
                title={p.archived ? "Bring back" : "Archive (keeps their history)"}
                onClick={() => archive.mutateAsync({ id: p.id, archived: !p.archived }).catch((err) => toast.error(err))}
              >
                {p.archived ? <ArchiveRestore size={16} /> : <Archive size={16} />}
              </button>
            </div>
          ))}
        </div>
        <form
          className="row"
          style={{ gap: 8 }}
          onSubmit={(e) => {
            e.preventDefault();
            add();
          }}
        >
          <input className="input grow" value={name} onChange={(e) => setName(e.target.value)} placeholder={people.length ? "Add someone" : "Start with yourself, e.g. Mom"} />
          <button className="btn btn-secondary" disabled={!name.trim() || create.isPending}>
            Add
          </button>
        </form>
      </div>
    </Modal>
  );
}
