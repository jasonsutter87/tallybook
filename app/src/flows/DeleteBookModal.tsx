import { Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useAction } from "../api";
import { Field, Modal, useToast } from "../components/ui";

interface DeleteReport {
  trashed: string[];
  keychain_entries_removed: number;
  bank_connections_revoked: number;
  warnings: string[];
}

/**
 * Deletes a book: revokes bank access, removes its keychain entries and
 * moves the file (optionally with its backups) to the Trash. The user must
 * type the business name to confirm.
 */
export default function DeleteBookModal(props: { open: boolean; onClose: () => void; path: string; name: string; onDeleted?: () => void }) {
  const toast = useToast();
  const del = useAction<DeleteReport>("book.delete");
  const [typed, setTyped] = useState("");
  const [withBackups, setWithBackups] = useState(false);

  useEffect(() => {
    if (props.open) {
      setTyped("");
      setWithBackups(false);
    }
  }, [props.open]);

  const matches = typed.trim() === props.name.trim();

  const submit = async () => {
    try {
      const r = await del.mutateAsync({ path: props.path, confirm_name: typed, include_backups: withBackups });
      const files = r.trashed.length;
      toast({ text: `${props.name} moved to the Trash${files > 1 ? ` (${files} files)` : ""}` });
      for (const w of r.warnings) toast.error(w);
      props.onClose();
      props.onDeleted?.();
    } catch (e) {
      toast.error(e);
    }
  };

  return (
    <Modal
      open={props.open}
      onClose={props.onClose}
      title={`Delete ${props.name}?`}
      footer={
        <>
          <button className="btn btn-secondary" onClick={props.onClose}>
            Cancel
          </button>
          <button className="btn btn-danger" disabled={!matches || del.isPending} onClick={submit}>
            <Trash2 size={15} /> {del.isPending ? "Deleting…" : "Delete book"}
          </button>
        </>
      }
    >
      <div className="stack">
        <div className="muted">This tears the book down:</div>
        <ul className="muted" style={{ margin: 0, paddingLeft: 20, lineHeight: 1.7 }}>
          <li>
            The book file moves to your <b>Trash</b>. Until you empty the Trash, you can put it back.
          </li>
          <li>Its bank connections are disconnected, and their keys are removed from your keychain.</li>
          <li>It's removed from your recent books.</li>
        </ul>
        <label className="row" style={{ gap: 10, cursor: "pointer" }}>
          <input type="checkbox" className="checkbox" checked={withBackups} onChange={(e) => setWithBackups(e.target.checked)} />
          <span>
            Also delete its backups <span className="subtle">(the dated copies in the Backups folder)</span>
          </span>
        </label>
        <Field label={<>Type <b>{props.name}</b> to confirm</>}>
          <input className="input" autoFocus value={typed} onChange={(e) => setTyped(e.target.value)} onKeyDown={(e) => e.key === "Enter" && matches && submit()} />
        </Field>
        <div className="subtle small" style={{ wordBreak: "break-all" }}>
          {props.path}
        </div>
      </div>
    </Modal>
  );
}
