import { useEffect } from "react";
import { call, queryClient } from "../api";
import type { CalendarSync } from "../types";

/** Actions that can change which bills are due: a bill made, paid or stopped, or a payment found. */
const CHANGES_BILLS = /^(recurring|bills|payments|inbox|entries|import)\.|^bank\.sync$/;
/** Waits this long after a change, so a burst of them makes one sync. */
const SETTLE_MS = 4_000;
/** Syncs at least this often, so the 90-day window moves along with the days. */
const EVERY_MS = 60 * 60 * 1000;

/**
 * Keeps the book's calendar (Settings → Calendar) up to date in the
 * background: soon after the book opens, after bill changes, and hourly.
 * Does nothing for a book with no calendar. Renders nothing.
 */
export default function AutoCalendarSync() {
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    let running = false;
    const sync = async () => {
      if (running) return schedule();
      running = true;
      try {
        const r = await call<CalendarSync>("calendar.sync");
        if (r.connected) void queryClient.invalidateQueries({ queryKey: ["calendar.status"] });
      } catch {
        // Quiet: Settings → Calendar shows the last error.
        void queryClient.invalidateQueries({ queryKey: ["calendar.status"] });
      } finally {
        running = false;
      }
    };
    const schedule = () => {
      clearTimeout(timer);
      timer = setTimeout(sync, SETTLE_MS);
    };
    const unsubscribe = queryClient.getMutationCache().subscribe((e) => {
      const method = e.mutation?.options.mutationKey?.[0];
      if (e.type === "updated" && e.mutation?.state.status === "success" && typeof method === "string" && CHANGES_BILLS.test(method)) schedule();
    });
    schedule();
    const hourly = setInterval(sync, EVERY_MS);
    return () => {
      unsubscribe();
      clearTimeout(timer);
      clearInterval(hourly);
    };
  }, []);
  return null;
}
