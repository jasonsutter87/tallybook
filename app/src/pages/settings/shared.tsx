// Pieces every settings card shares: the pref form, the card frame, the Save/Discard
// footer and the save-prefs hook.

import type { ReactNode } from "react";
import { useAction } from "../../api";
import { useToast } from "../../components/ui";

export interface FormApi {
  get: (key: string, fallback?: string) => string;
  set: (key: string, value: string) => void;
  dirty: (keys: string[], fallbacks?: Record<string, string>) => boolean;
  reset: (keys: string[]) => void;
}

export function SettingsCard(props: { id: string; icon: ReactNode; title: string; desc: ReactNode; children: ReactNode; foot?: ReactNode }) {
  return (
    <section className="card settings-card" id={props.id}>
      <div className="card-header">
        <div className="settings-card-head">
          <span className="icon-bubble">{props.icon}</span>
          <div>
            <h3 className="card-title">{props.title}</h3>
            <div className="settings-card-desc">{props.desc}</div>
          </div>
        </div>
      </div>
      <div className="card-body" style={{ paddingTop: 20 }}>
        {props.children}
      </div>
      {props.foot && <div className="settings-foot">{props.foot}</div>}
    </section>
  );
}

/** Save/Discard footer shared by the pref cards. */
export function SaveFoot(props: { dirty: boolean; busy: boolean; onSave: () => void; onDiscard: () => void }) {
  return (
    <>
      {props.dirty ? <span className="subtle small grow">You have unsaved changes</span> : <span className="subtle small grow">All changes saved</span>}
      {props.dirty && (
        <button className="btn btn-ghost btn-sm" onClick={props.onDiscard} disabled={props.busy}>
          Discard
        </button>
      )}
      <button className="btn btn-primary btn-sm" onClick={props.onSave} disabled={!props.dirty || props.busy}>
        {props.busy ? "Saving…" : "Save changes"}
      </button>
    </>
  );
}

export function useSavePrefs(form: FormApi, keys: string[], label: string) {
  const toast = useToast();
  const save = useAction<void, unknown>("settings.set");
  const run = async (prefs: Record<string, string | null>) => {
    try {
      await save.mutateAsync({ prefs });
      form.reset(keys);
      toast({ text: `${label} saved` });
    } catch (e) {
      toast.error(e);
    }
  };
  return { run, busy: save.isPending };
}
