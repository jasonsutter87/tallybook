// Signed self-updates (desktop only). The app checks GitHub Releases'
// latest.json; Tauri verifies the download against the public key in
// tauri.conf.json before installing anything.

import { useCallback, useEffect, useRef, useState } from "react";
import { call, isTauri } from "./api";

export type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "none" }
  | { kind: "available"; version: string; notes: string; date: string | null }
  | { kind: "downloading"; version: string; percent: number | null }
  | { kind: "installing"; version: string }
  | { kind: "error"; message: string };

const CHECK_EVERY_MS = 6 * 60 * 60 * 1000;
const FIRST_CHECK_DELAY_MS = 8_000;
const SNOOZE_KEY = "tallybook.update.snoozed";

type Update = Awaited<ReturnType<typeof import("@tauri-apps/plugin-updater").check>>;

function snoozed(version: string): boolean {
  try {
    return localStorage.getItem(SNOOZE_KEY) === version;
  } catch {
    return false;
  }
}

export function snooze(version: string) {
  try {
    localStorage.setItem(SNOOZE_KEY, version);
  } catch {
    /* storage unavailable: the banner just comes back next launch */
  }
}

export async function currentVersion(): Promise<string | null> {
  if (!isTauri) return null;
  const { getVersion } = await import("@tauri-apps/api/app");
  return getVersion();
}

/**
 * Update state plus actions. `auto` runs a quiet check after launch and
 * every few hours; failures in quiet checks are swallowed (offline, or the
 * repository isn't public yet) and only surface on a manual check.
 */
export function useUpdater(auto: boolean) {
  const [state, setState] = useState<UpdateState>({ kind: "idle" });
  const update = useRef<Update>(null);

  const check = useCallback(async (manual: boolean) => {
    if (!isTauri) {
      if (manual) setState({ kind: "error", message: "Updates are available in the desktop app." });
      return;
    }
    setState({ kind: "checking" });
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const found = await check();
      update.current = found;
      if (!found) return setState({ kind: "none" });
      if (!manual && snoozed(found.version)) return setState({ kind: "idle" });
      setState({ kind: "available", version: found.version, notes: found.body ?? "", date: found.date ?? null });
    } catch (e) {
      setState(manual ? { kind: "error", message: `Couldn't check for updates: ${String(e)}` } : { kind: "idle" });
    }
  }, []);

  const install = useCallback(async () => {
    const found = update.current;
    if (!found) return;
    try {
      // A full copy of the open book first: new versions may upgrade the file.
      const status = await call<{ open: boolean }>("book.status");
      if (status.open) await call("book.backup");

      let total = 0;
      let received = 0;
      setState({ kind: "downloading", version: found.version, percent: null });
      await found.downloadAndInstall((event) => {
        if (event.event === "Started") total = event.data.contentLength ?? 0;
        if (event.event === "Progress") {
          received += event.data.chunkLength;
          setState({ kind: "downloading", version: found.version, percent: total ? Math.min(100, Math.round((received / total) * 100)) : null });
        }
        if (event.event === "Finished") setState({ kind: "installing", version: found.version });
      });
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    } catch (e) {
      setState({ kind: "error", message: `The update didn't install: ${String(e)}` });
    }
  }, []);

  useEffect(() => {
    if (!auto || !isTauri) return;
    const first = setTimeout(() => void check(false), FIRST_CHECK_DELAY_MS);
    const every = setInterval(() => void check(false), CHECK_EVERY_MS);
    return () => {
      clearTimeout(first);
      clearInterval(every);
    };
  }, [auto, check]);

  return { state, check, install, dismiss: () => setState({ kind: "idle" }) };
}
