// Cloud copies card: encrypted backups to a folder a sync service watches.

import { Cloud, GitBranch, ShieldCheck } from "lucide-react";
import { useState } from "react";
import { isTauri, useAction, useApi } from "../../api";
import { Loading, useToast } from "../../components/ui";
import type { CloudBackupReport, CloudStatus } from "../../types";
import { SettingsCard } from "./shared";

async function pickFolder(): Promise<string | null> {
  if (!isTauri) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const r = await open({ directory: true, multiple: false });
  return typeof r === "string" ? r : null;
}

export default function CloudCard() {
  const toast = useToast();
  const status = useApi<CloudStatus>("cloud.status");
  const set = useAction("cloud.set");
  const backup = useAction<CloudBackupReport>("cloud.backup_now");
  const [typedFolder, setTypedFolder] = useState("");
  const [typedRepo, setTypedRepo] = useState("");
  const s = status.data;

  const save = async (patch: Record<string, unknown>) => {
    try {
      await set.mutateAsync(patch);
    } catch (e) {
      toast.error(e);
    }
  };

  const destination = (kind: "folder" | "git", value: string | null) => {
    const isGit = kind === "git";
    const typed = isGit ? typedRepo : typedFolder;
    const setTyped = isGit ? setTypedRepo : setTypedFolder;
    return (
      <div className="stack-sm">
        <div className="label row" style={{ gap: 6 }}>
          {isGit ? <GitBranch size={14} /> : <Cloud size={14} />}
          {isGit ? "Git repository" : "Synced folder"}
        </div>
        {value ? (
          <div className="row" style={{ gap: 8 }}>
            <div className="path-box grow" style={{ margin: 0 }}>
              {value}
            </div>
            <button className="btn btn-ghost btn-sm" onClick={() => save({ [kind]: null })}>
              Remove
            </button>
          </div>
        ) : (
          <>
            {!isGit && !!s?.suggestions.length && (
              <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
                {s.suggestions.map((g) => (
                  <button key={g.path} className="btn btn-secondary btn-sm" onClick={() => save({ folder: g.path })}>
                    <Cloud size={13} /> {g.label}
                  </button>
                ))}
              </div>
            )}
            <div className="row" style={{ gap: 8 }}>
              {isTauri ? (
                <button
                  className="btn btn-secondary btn-sm"
                  onClick={async () => {
                    const p = await pickFolder();
                    if (p) await save({ [kind]: p });
                  }}
                >
                  Choose {isGit ? "repository" : "folder"}…
                </button>
              ) : (
                <>
                  <input className="input grow" placeholder={isGit ? "/path/to/cloned/repo" : "/path/to/synced/folder"} value={typed} onChange={(e) => setTyped(e.target.value)} />
                  <button className="btn btn-secondary btn-sm" disabled={!typed.trim()} onClick={() => save({ [kind]: typed.trim() })}>
                    Use
                  </button>
                </>
              )}
            </div>
            <div className="hint">
              {isGit
                ? "A repository you've already cloned (ideally private). Tallybook commits the encrypted book and pushes with your own git login."
                : "Pick your Google Drive, iCloud Drive, Dropbox or OneDrive folder. Their desktop apps upload what Tallybook saves there."}
            </div>
          </>
        )}
        {isGit && value && !s?.git_has_remote && <div className="hint">This repository has no remote yet, so copies are committed locally only.</div>}
      </div>
    );
  };

  return (
    <SettingsCard
      id="cloud"
      icon={<Cloud size={18} />}
      title="Cloud copies"
      desc="Keep encrypted copies off this computer. Only encrypted books can be copied, so nothing readable ever leaves your machine."
    >
      {!s ? (
        <Loading />
      ) : !s.encrypted ? (
        <div className="banner warn">
          <ShieldCheck size={18} /> Turn on encryption above first. Cloud copies are always encrypted.
        </div>
      ) : (
        <div className="stack" style={{ gap: 20 }}>
          {destination("folder", s.folder)}
          {destination("git", s.git)}
          <div className="divider" style={{ margin: 0 }} />
          <div className="row" style={{ gap: 16, flexWrap: "wrap" }}>
            <label className="switch">
              <input type="checkbox" checked={s.auto} onChange={(e) => save({ auto: e.target.checked })} />
              Copy automatically once a day
            </label>
            <label className="row small muted" style={{ gap: 8 }}>
              Keep the last
              <select className="select" style={{ width: 80, height: 32 }} value={s.keep} onChange={(e) => save({ keep: Number(e.target.value) })}>
                {[3, 5, 10, 20, 50].map((n) => (
                  <option key={n} value={n}>
                    {n}
                  </option>
                ))}
              </select>
              copies in the folder
            </label>
          </div>
          <div className="row" style={{ gap: 12, flexWrap: "wrap" }}>
            <button
              className="btn btn-primary"
              disabled={backup.isPending || (!s.folder && !s.git)}
              onClick={async () => {
                try {
                  const r = await backup.mutateAsync(null);
                  for (const [label, d] of [["Folder", r.folder], ["Git", r.git]] as const) {
                    if (!d) continue;
                    if (d.ok) toast({ text: `${label}: copied${d.detail ? `. ${d.detail}` : ""}` });
                    else toast.error(`${label}: ${d.detail}`);
                  }
                } catch (e) {
                  toast.error(e);
                }
              }}
            >
              <Cloud size={16} /> {backup.isPending ? "Copying…" : "Copy now"}
            </button>
            <span className="muted small">
              {s.last ? `Last copy ${new Date(s.last).toLocaleString()}` : "No copies yet"}
              {s.last_error && <span className="neg"> · {s.last_error}</span>}
            </span>
          </div>
          <div className="hint">To restore: open the copy in Tallybook (Welcome → Open a book) and enter your recovery key.</div>
        </div>
      )}
    </SettingsCard>
  );
}
