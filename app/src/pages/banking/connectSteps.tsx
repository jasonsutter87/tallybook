// The connect modal's cancelled notice. The state and the buttons stay in
// ConnectModal.

import { XCircle } from "lucide-react";

export function CancelledStep() {
  return (
    <div className="bk-wait">
      <div className="bk-wait-ring" style={{ animation: "none", background: "var(--surface-hover)", color: "var(--ink-3)" }}>
        <XCircle size={26} />
      </div>
      <div className="bold" style={{ fontSize: 17 }}>
        Cancelled
      </div>
      <div className="muted small" style={{ marginTop: 6 }}>
        The bank connection wasn't finished. You can try again whenever you're ready.
      </div>
    </div>
  );
}
