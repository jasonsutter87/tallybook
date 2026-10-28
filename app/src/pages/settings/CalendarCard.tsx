// Calendar card: opt in to bill reminders on Google Calendar or Outlook.

import { CalendarCheck, ExternalLink, RefreshCw } from "lucide-react";
import { useRef, useState } from "react";
import { call, openExternal, queryClient, useAction, useApi } from "../../api";
import { Loading, Spinner, useToast } from "../../components/ui";
import type { CalendarProvider, CalendarStatus, CalendarSync } from "../../types";
import { SettingsCard } from "./shared";

const NAMES: Record<CalendarProvider, string> = { google: "Google Calendar", outlook: "Outlook" };
const REMINDERS = [
  { days: 0, label: "No reminder" },
  { days: 1, label: "The day before" },
  { days: 2, label: "2 days before" },
  { days: 3, label: "3 days before" },
  { days: 7, label: "A week before" },
];

function synced(r: CalendarSync): string {
  if (!r.connected) return "No calendar connected";
  const parts = [r.added && `${r.added} added`, r.updated && `${r.updated} updated`, r.removed && `${r.removed} removed`].filter(Boolean);
  return parts.length ? `Calendar updated: ${parts.join(", ")}` : "Calendar is up to date";
}

export default function CalendarCard() {
  const toast = useToast();
  const status = useApi<CalendarStatus>("calendar.status");
  const set = useAction("calendar.set");
  const [waiting, setWaiting] = useState<CalendarProvider | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [confirmOff, setConfirmOff] = useState(false);
  // Set when the user cancels a sign-in, so the error that ends it isn't shown.
  const cancelled = useRef(false);
  const s = status.data;

  const refresh = () => queryClient.invalidateQueries({ queryKey: ["calendar.status"] });

  const sync = async (quiet = false) => {
    setSyncing(true);
    try {
      const r = await call<CalendarSync>("calendar.sync");
      if (!quiet) toast({ text: synced(r) });
    } catch (e) {
      toast.error(e);
    } finally {
      setSyncing(false);
      void refresh();
    }
  };

  const connect = async (provider: CalendarProvider) => {
    cancelled.current = false;
    try {
      const { url } = await call<{ url: string }>("calendar.connect_start", { provider });
      setWaiting(provider);
      await openExternal(url);
      const r = await call<CalendarSync>("calendar.connect_finish");
      toast({ text: `Connected to ${NAMES[provider]}. ${synced(r)}` });
    } catch (e) {
      // Cancelling here already said so; anything else is news.
      if (!cancelled.current) toast.error(e);
    } finally {
      setWaiting(null);
      void refresh();
    }
  };

  const cancel = async () => {
    cancelled.current = true;
    await call("calendar.connect_cancel").catch(() => {});
  };

  const change = async (patch: { show_amounts?: boolean; remind_days?: number }) => {
    try {
      await set.mutateAsync(patch);
      await sync(true);
    } catch (e) {
      toast.error(e);
    }
  };

  const disconnect = async () => {
    try {
      const r = await call<{ warnings: string[] }>("calendar.disconnect");
      if (r.warnings.length) r.warnings.forEach((w) => toast.error(w));
      else toast({ text: "Calendar disconnected and the Tallybook bills calendar removed" });
    } catch (e) {
      toast.error(e);
    } finally {
      setConfirmOff(false);
      void refresh();
    }
  };

  let body;
  if (!s) {
    body = <Loading />;
  } else if (waiting) {
    body = (
      <div className="stack-sm">
        <div className="row" style={{ gap: 10 }}>
          <Spinner />
          <span>Finish signing in to {NAMES[waiting]} in your browser…</span>
        </div>
        <div className="hint">Tallybook asks only to manage a calendar of its own. It can't see your other events.</div>
        <div>
          <button className="btn btn-ghost btn-sm" onClick={cancel}>
            Cancel
          </button>
        </div>
      </div>
    );
  } else if (!s.connected) {
    body = (
      <div className="stack-sm">
        {s.available.length ? (
          <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
            {s.available.map((p) => (
              <button key={p} className="btn btn-secondary" onClick={() => connect(p)}>
                <ExternalLink size={15} /> Connect {NAMES[p]}
              </button>
            ))}
          </div>
        ) : (
          <div className="banner">Calendar sign-in isn't switched on in this version of Tallybook yet.</div>
        )}
        <div className="hint">
          Tallybook makes a calendar called "Tallybook bills" and adds an all-day event for each bill due in the next {s.horizon_days} days, from
          your household bills and the business bills you owe. Paid bills come off on their own.
        </div>
      </div>
    );
  } else {
    body = (
      <div className="stack" style={{ gap: 16 }}>
        <div className="row" style={{ gap: 12, flexWrap: "wrap" }}>
          <span>
            Connected to <strong>{s.provider ? NAMES[s.provider] : "a calendar"}</strong>
          </span>
          <span className="muted small">
            {s.last ? `Last updated ${new Date(s.last).toLocaleString()}` : "Not updated yet"}
            {s.last_error && <span className="neg"> · {s.last_error}</span>}
          </span>
        </div>
        <div className="row" style={{ gap: 16, flexWrap: "wrap" }}>
          <label className="switch">
            <input type="checkbox" checked={s.show_amounts} disabled={set.isPending} onChange={(e) => change({ show_amounts: e.target.checked })} />
            Show amounts on events
          </label>
          <label className="row small muted" style={{ gap: 8 }}>
            Remind me
            <select
              className="select"
              style={{ width: 160, height: 32 }}
              value={s.remind_days}
              disabled={set.isPending}
              onChange={(e) => change({ remind_days: Number(e.target.value) })}
            >
              {REMINDERS.map((r) => (
                <option key={r.days} value={r.days}>
                  {r.label}
                </option>
              ))}
            </select>
            {s.remind_days > 0 && "at 9 am"}
          </label>
        </div>
        <div className="hint">Calendars are often shared, so amounts are off unless you turn them on.</div>
        <div className="row" style={{ gap: 12, flexWrap: "wrap" }}>
          <button className="btn btn-primary" disabled={syncing} onClick={() => sync()}>
            <RefreshCw size={16} /> {syncing ? "Updating…" : "Update now"}
          </button>
          {confirmOff ? (
            <>
              <span className="small">Remove the Tallybook bills calendar?</span>
              <button className="btn btn-danger btn-sm" onClick={disconnect}>
                Disconnect
              </button>
              <button className="btn btn-ghost btn-sm" onClick={() => setConfirmOff(false)}>
                Keep it
              </button>
            </>
          ) : (
            <button className="btn btn-ghost" onClick={() => setConfirmOff(true)}>
              Disconnect
            </button>
          )}
        </div>
      </div>
    );
  }

  return (
    <SettingsCard
      id="calendar"
      icon={<CalendarCheck size={18} />}
      title="Calendar"
      desc="Optional: put the bills you have coming due on your Google or Outlook calendar, so they show up on your phone too. Tallybook updates it when your bills change."
    >
      {body}
    </SettingsCard>
  );
}
