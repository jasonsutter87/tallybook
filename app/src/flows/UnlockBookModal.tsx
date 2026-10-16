import { KeyRound } from "lucide-react";
import { useEffect, useState } from "react";
import { useAction } from "../api";
import { ErrorBanner, Field, Modal } from "../components/ui";

/** Asks for a recovery key to open an encrypted book this machine has no key for. */
export default function UnlockBookModal(props: { path: string | null; onClose: () => void }) {
  const unlock = useAction("book.unlock");
  const [key, setKey] = useState("");
  const [remember, setRemember] = useState(true);

  useEffect(() => {
    setKey("");
    unlock.reset();
  }, [props.path]); // start fresh whenever a different book is asked for

  const submit = async () => {
    if (!props.path || !key.trim()) return;
    try {
      await unlock.mutateAsync({ path: props.path, recovery_key: key, remember });
      props.onClose();
    } catch {
      /* shown below */
    }
  };

  return (
    <Modal
      open={!!props.path}
      onClose={props.onClose}
      title="This book is encrypted"
      subtitle="Enter its recovery key to open it on this computer."
      footer={
        <>
          <button className="btn btn-secondary" onClick={props.onClose}>
            Cancel
          </button>
          <button className="btn btn-primary" disabled={!key.trim() || unlock.isPending} onClick={submit}>
            <KeyRound size={15} /> {unlock.isPending ? "Unlocking…" : "Unlock"}
          </button>
        </>
      }
    >
      <div className="stack">
        <ErrorBanner error={unlock.error} />
        <Field label="Recovery key" hint="It starts with TB1-. Dashes, spaces and capitals don't matter.">
          <textarea
            className="textarea"
            autoFocus
            spellCheck={false}
            style={{ fontFamily: "var(--mono)", minHeight: 72 }}
            value={key}
            onChange={(e) => setKey(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                submit();
              }
            }}
            placeholder="TB1-XXXXX-XXXXX-…"
          />
        </Field>
        <label className="switch">
          <input type="checkbox" checked={remember} onChange={(e) => setRemember(e.target.checked)} />
          Remember on this computer (saved in the system keychain)
        </label>
        <div className="subtle small" style={{ wordBreak: "break-all" }}>
          {props.path}
        </div>
      </div>
    </Modal>
  );
}
