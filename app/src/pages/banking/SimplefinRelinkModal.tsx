// Reconnect a SimpleFIN connection with a new setup token.

import { useState } from "react";
import { call, queryClient } from "../../api";
import { ErrorBanner, Modal, useToast } from "../../components/ui";
import type { Connection } from "../../types";

export default function SimplefinRelinkModal(props: { conn: Connection; onClose: () => void }) {
  const toast = useToast();
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      await call("bank.simplefin_reconnect", { connection_id: props.conn.id, setup_token: token.trim() });
      try {
        await call("bank.sync", { connection_id: props.conn.id });
      } catch {
        // The sync result shows on the card; reconnecting itself worked.
      }
      await queryClient.invalidateQueries();
      toast({ text: `${props.conn.institution} reconnected` });
      props.onClose();
    } catch (e) {
      setError(e as Error);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal
      open
      onClose={props.onClose}
      title={`Reconnect ${props.conn.institution}`}
      subtitle="Your linked accounts and history stay as they are."
      footer={
        <>
          <button className="btn btn-secondary" onClick={props.onClose}>
            Cancel
          </button>
          <button className="btn btn-primary" disabled={!token.trim() || busy} onClick={submit}>
            {busy ? "Reconnecting…" : "Reconnect"}
          </button>
        </>
      }
    >
      <div className="stack">
        <ErrorBanner error={error} />
        <div className="muted">
          In SimpleFIN Bridge, create a new setup token for Tallybook and paste it here. It replaces the expired access for this connection.
        </div>
        <textarea className="textarea" autoFocus value={token} onChange={(e) => setToken(e.target.value)} placeholder="Paste your SimpleFIN setup token" />
      </div>
    </Modal>
  );
}
