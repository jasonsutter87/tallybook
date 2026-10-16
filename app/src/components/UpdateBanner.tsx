import { Download, RefreshCw, Sparkles, X } from "lucide-react";
import { useState } from "react";
import { snooze, useUpdater } from "../updater";
import { Modal } from "./ui";

/** Floating card in the corner when a new version is ready. Desktop only. */
export default function UpdateBanner() {
  const { state, install, dismiss } = useUpdater(true);
  const [notesOpen, setNotesOpen] = useState(false);

  if (state.kind !== "available" && state.kind !== "downloading" && state.kind !== "installing" && state.kind !== "error") return null;
  // Quiet checks never set "error"; only a failed install does.
  const version = "version" in state ? state.version : "";
  const busy = state.kind === "downloading" || state.kind === "installing";

  return (
    <div
      className="card no-print"
      role="status"
      style={{
        position: "fixed",
        right: 24,
        bottom: 24,
        zIndex: 60,
        width: 340,
        padding: "16px 18px",
        boxShadow: "var(--shadow-lg)",
      }}
    >
      <div className="row" style={{ alignItems: "flex-start", gap: 12 }}>
        <span className="icon-bubble">{busy ? <RefreshCw size={18} className="spin" /> : <Sparkles size={18} />}</span>
        <div className="grow">
          {state.kind === "error" ? (
            <>
              <div className="bold">Update didn't finish</div>
              <div className="muted small" style={{ marginTop: 2 }}>
                {state.message}. Your books weren't changed.
              </div>
            </>
          ) : (
            <>
              <div className="bold">
                {state.kind === "available" && `Tallybook ${version} is ready`}
                {state.kind === "downloading" && `Downloading ${version}…`}
                {state.kind === "installing" && "Installing — restarting in a moment"}
              </div>
              <div className="muted small" style={{ marginTop: 2 }}>
                {state.kind === "available" && "We'll back up your book, update, and restart. Takes about a minute."}
                {state.kind === "downloading" && (state.percent != null ? `${state.percent}%` : "Starting…")}
              </div>
              {state.kind === "downloading" && (
                <div className="progress" style={{ marginTop: 10 }}>
                  <div style={{ width: `${state.percent ?? 5}%`, transition: "width .2s" }} />
                </div>
              )}
            </>
          )}
        </div>
        {!busy && (
          <button
            className="btn btn-ghost btn-icon btn-sm"
            aria-label="Not now"
            onClick={() => {
              if (state.kind === "available") snooze(version);
              dismiss();
            }}
          >
            <X size={16} />
          </button>
        )}
      </div>
      {state.kind === "available" && (
        <div className="row" style={{ justifyContent: "flex-end", marginTop: 14, gap: 8 }}>
          {state.notes && (
            <button className="btn btn-ghost btn-sm" onClick={() => setNotesOpen(true)}>
              What's new
            </button>
          )}
          <button className="btn btn-primary btn-sm" onClick={install}>
            <Download size={14} /> Update now
          </button>
        </div>
      )}
      {state.kind === "available" && (
        <Modal open={notesOpen} onClose={() => setNotesOpen(false)} title={`What's new in ${version}`}>
          <div className="muted" style={{ whiteSpace: "pre-wrap" }}>
            {state.notes}
          </div>
        </Modal>
      )}
    </div>
  );
}
