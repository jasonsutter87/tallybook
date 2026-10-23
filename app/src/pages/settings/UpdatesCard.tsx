// Updates card: check for and install a new version of the app.

import { Download } from "lucide-react";
import { useEffect, useState } from "react";
import { currentVersion, useUpdater } from "../../updater";
import { SettingsCard } from "./shared";

export default function UpdatesCard() {
  const { state, check, install } = useUpdater(false);
  const [version, setVersion] = useState<string | null>(null);
  useEffect(() => {
    currentVersion().then(setVersion, () => setVersion(null));
  }, []);
  return (
    <SettingsCard
      id="updates"
      icon={<Download size={18} />}
      title="Updates"
      desc="Tallybook checks for new versions a few times a day. Every update is signed, and your book is backed up before it installs."
    >
      <div className="stack">
        <div className="row" style={{ gap: 12, flexWrap: "wrap" }}>
          <span className="pill">{version ? `Version ${version}` : "Browser preview"}</span>
          {state.kind === "available" ? (
            <button className="btn btn-primary" onClick={install}>
              <Download size={16} /> Update to {state.version}
            </button>
          ) : (
            <button className="btn btn-secondary" disabled={state.kind === "checking" || state.kind === "downloading" || state.kind === "installing"} onClick={() => check(true)}>
              {state.kind === "checking" ? "Checking…" : "Check for updates"}
            </button>
          )}
          {state.kind === "none" && <span className="muted small">You're on the latest version.</span>}
          {state.kind === "downloading" && <span className="muted small">Downloading{state.percent != null ? ` · ${state.percent}%` : "…"}</span>}
          {state.kind === "installing" && <span className="muted small">Installing — Tallybook will restart.</span>}
        </div>
        {state.kind === "error" && <div className="banner error">{state.message}</div>}
      </div>
    </SettingsCard>
  );
}
