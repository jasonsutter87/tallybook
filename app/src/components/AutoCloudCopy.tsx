import { useEffect, useRef } from "react";
import { call } from "../api";
import type { CloudBackupReport, CloudStatus } from "../types";
import { useToast } from "./ui";

/** Makes the daily encrypted cloud copy in the background when one is due. Renders nothing. */
export default function AutoCloudCopy() {
  const toast = useToast();
  const ran = useRef(false);
  useEffect(() => {
    if (ran.current) return;
    ran.current = true;
    const timer = setTimeout(async () => {
      try {
        const status = await call<CloudStatus>("cloud.status");
        if (!status.due) return;
        const r = await call<CloudBackupReport>("cloud.backup_now");
        const failed = [r.folder, r.git].find((d) => d && !d.ok);
        if (failed) toast.error(`Automatic cloud copy didn't finish: ${failed.detail}`);
      } catch {
        /* quiet: the Settings card shows the last error */
      }
    }, 15_000);
    return () => clearTimeout(timer);
  }, [toast]);
  return null;
}
