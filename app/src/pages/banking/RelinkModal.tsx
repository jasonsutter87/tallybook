// Reconnect a Plaid connection that needs a fresh login.

import { RefreshCw } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { call, openExternal, queryClient } from "../../api";
import { ErrorBanner, Loading, Modal, useToast } from "../../components/ui";
import type { Connection, HostedLink } from "../../types";
import { usePlaidPoll, Waiting } from "./PlaidPoll";

export default function RelinkModal(props: { conn: Connection; onClose: () => void }) {
  const toast = useToast();
  const [link, setLink] = useState<HostedLink | null>(null);
  const [state, setState] = useState<"starting" | "wait" | "exited">("starting");
  const [error, setError] = useState<Error | null>(null);

  const start = async () => {
    setError(null);
    setState("starting");
    try {
      const l = await call<HostedLink>("bank.plaid_relink_start", { connection_id: props.conn.id });
      setLink(l);
      setState("wait");
      await openExternal(l.url);
    } catch (e) {
      setError(e as Error);
      setState("exited");
    }
  };

  // Start once (StrictMode runs effects twice in development; don't open two browser tabs).
  const started = useRef(false);
  useEffect(() => {
    if (started.current) return;
    started.current = true;
    void start();
  });

  usePlaidPoll(
    state === "wait" ? (link?.link_token ?? null) : null,
    props.conn.id,
    async (r) => {
      if (r.status === "exited") return setState("exited");
      try {
        await call("bank.sync", { connection_id: props.conn.id });
      } catch {
        // The sync result shows on the card; reconnecting itself worked.
      }
      await queryClient.invalidateQueries();
      toast({ text: `${props.conn.institution} reconnected` });
      props.onClose();
    },
    (e) => {
      setError(e as Error);
      setState("exited");
    },
  );

  return (
    <Modal
      open
      onClose={props.onClose}
      title={`Reconnect ${props.conn.institution}`}
      footer={
        state === "exited" ? (
          <>
            <button className="btn btn-secondary" onClick={props.onClose}>
              Close
            </button>
            <button className="btn btn-primary" onClick={start}>
              <RefreshCw size={16} /> Try again
            </button>
          </>
        ) : undefined
      }
    >
      <ErrorBanner error={error} />
      {state === "starting" && <Loading label="Opening Plaid…" />}
      {state === "wait" && link && <Waiting url={link.url} onCancel={props.onClose} />}
      {state === "exited" && !error && <div className="muted">The sign-in wasn't finished. Try again whenever you're ready.</div>}
    </Modal>
  );
}
