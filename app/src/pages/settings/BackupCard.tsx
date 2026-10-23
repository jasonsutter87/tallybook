// Backup card: save a dated copy of the book next to it.

import { DatabaseBackup } from "lucide-react";
import { useState } from "react";
import { useAction } from "../../api";
import { useToast } from "../../components/ui";
import { SettingsCard } from "./shared";

export default function BackupCard() {
  const toast = useToast();
  const backup = useAction<string, unknown>("book.backup");
  const [last, setLast] = useState<string | null>(null);
  return (
    <SettingsCard
      id="backup"
      icon={<DatabaseBackup size={18} />}
      title="Backup"
      desc="A backup is a full copy of your book file, saved in a Backups folder right next to it."
    >
      <div className="stack">
        <div className="row" style={{ gap: 12, flexWrap: "wrap" }}>
          <button
            className="btn btn-primary"
            disabled={backup.isPending}
            onClick={async () => {
              try {
                const path = await backup.mutateAsync(null);
                setLast(path);
                toast({ text: `Backed up to ${path}` });
              } catch (e) {
                toast.error(e);
              }
            }}
          >
            <DatabaseBackup size={16} /> {backup.isPending ? "Backing up…" : "Back up now"}
          </button>
          <span className="muted small">Takes a second. Each backup is dated, so older ones are never overwritten.</span>
        </div>
        {last && (
          <div>
            <div className="label" style={{ marginBottom: 6 }}>
              Latest backup
            </div>
            <div className="path-box">{last}</div>
          </div>
        )}
        <div className="hint">To restore, open a backup file from the Welcome screen like any other book. Copying your book file to a USB drive or cloud folder works too.</div>
      </div>
    </SettingsCard>
  );
}
