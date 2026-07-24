// src/hooks/useAcquisitionWatch.ts
//
// useAcquisitionWatch — continuous WARN-mode watcher for forensic
// acquisition tooling. REUSES the already-shipped read-only
// `Scan-AcquisitionThreats` command (see useBackend().scanAcquisitionThreats)
// — no new Pro code, no auto-lockdown. Same interval-poll shape as
// useShieldQuotaTicker: when enabled AND entitled, poll every ~60s; on a
// hit, warn once per newly-seen driver/process name so the same finding
// doesn't re-toast every cycle. Clears the interval on cleanup.

import { useEffect, useRef } from "react";
import { useBackend } from "./useBackend";
import { recordEvidence } from "../lib/evidence";
import { showWarning } from "../utils/toast";

const POLL_INTERVAL_MS = 60_000;

export default function useAcquisitionWatch(enabled: boolean, hasPaid: boolean) {
  const { scanAcquisitionThreats } = useBackend();
  const seenRef = useRef<Set<string>>(new Set());

  useEffect(() => {
    if (!enabled || !hasPaid) {
      seenRef.current.clear();
      return;
    }

    let cancelled = false;
    const tick = async () => {
      if (cancelled) return;
      try {
        const res = await scanAcquisitionThreats();
        if (cancelled || !res.success || !res.data) return;
        const { detected, drivers = [], processes = [] } = res.data as {
          detected: boolean;
          drivers?: string[];
          processes?: string[];
        };
        if (!detected) return;

        // De-dup: only warn on names not already seen this session.
        const names = [...drivers, ...processes];
        const fresh = names.filter((name) => !seenRef.current.has(name));
        if (fresh.length === 0) return;
        for (const name of fresh) seenRef.current.add(name);

        recordEvidence(
          "monitor",
          "warn",
          `Acquisition tooling detected: ${fresh.join(", ")}`,
        );
        showWarning(
          `Possible forensic acquisition tooling detected: ${fresh.join(", ")}. Investigate before continuing.`,
          10_000,
        );
      } catch (err) {
        console.warn("[useAcquisitionWatch] scan failed:", err);
      }
    };

    void tick();
    const interval = setInterval(tick, POLL_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, [enabled, hasPaid, scanAcquisitionThreats]);
}
