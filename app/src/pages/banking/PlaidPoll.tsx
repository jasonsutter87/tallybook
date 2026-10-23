// Waiting for Plaid's Hosted Link in the system browser: the poll and the
// "waiting" panel.

import { ExternalLink } from "lucide-react";
import { useEffect, useRef } from "react";
import { call, openExternal } from "../../api";
import type { PollResult } from "../../types";

/** Polls Plaid Hosted Link every 2s until the user finishes or exits. */
export function usePlaidPoll(linkToken: string | null, connectionId: number | undefined, onResult: (r: PollResult) => void, onError: (e: unknown) => void) {
  const cb = useRef({ onResult, onError });
  cb.current = { onResult, onError };
  useEffect(() => {
    if (!linkToken) return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    const tick = async () => {
      try {
        const r = await call<PollResult>("bank.plaid_poll", { link_token: linkToken, connection_id: connectionId ?? null });
        if (stopped) return;
        if (r.status === "waiting") timer = setTimeout(tick, 2000);
        else cb.current.onResult(r);
      } catch (e) {
        if (!stopped) cb.current.onError(e);
      }
    };
    timer = setTimeout(tick, 2000);
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [linkToken, connectionId]);
}

export function Waiting(props: { onCancel: () => void; url: string }) {
  return (
    <div className="bk-wait">
      <div className="bk-wait-ring">
        <ExternalLink size={26} />
      </div>
      <div className="bold" style={{ fontSize: 17 }}>
        Finish connecting in your browser…
      </div>
      <div className="muted small" style={{ margin: "6px auto 16px", maxWidth: 360 }}>
        Sign in to your bank on the Plaid page that just opened. This window updates on its own when you're done.
      </div>
      <div className="row" style={{ justifyContent: "center" }}>
        <button className="btn btn-ghost btn-sm" onClick={() => openExternal(props.url)}>
          Open the page again
        </button>
        <button className="btn btn-secondary btn-sm" onClick={props.onCancel}>
          Cancel
        </button>
      </div>
    </div>
  );
}
