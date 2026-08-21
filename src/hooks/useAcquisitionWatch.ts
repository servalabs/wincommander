// src/hooks/useAcquisitionWatch.ts
//
// useAcquisitionWatch — WARN + auto-dismount watcher for forensic
// acquisition tooling. REUSES the already-shipped read-only
// `Scan-AcquisitionThreats` command (see useBackend().scanAcquisitionThreats)
// — no new Pro code. Same interval-poll shape as useShieldQuotaTicker: when
// enabled AND entitled, poll every ~60s; on a hit, warn once per newly-seen
// driver/process name so the same finding doesn't re-toast every cycle.
//
// Auto-response: on a FRESH detection (never on an already-seen finding, so
// this fires once per new threat batch, not every poll tick), dismount all
// encrypted volumes via the existing Dismount-AllEncryptionVolumes command.
// This is a REVERSIBLE response only — it unmounts, it does not erase any
// key or touch the vault contents; re-entering the password remounts it
// normally. Deliberately does NOT escalate to lockdown/self-destruct or any
// key-erasing command — those require their own explicit trigger, per
// AGENTS.md's rule that the lockdown cascade is never invoked inline.
// Clears the interval on cleanup.

import { useEffect, useRef } from "react";
import { useBackend } from "./useBackend";
import { recordEvidence } from "../lib/evidence";
import { showWarning, showError } from "../utils/toast";

const POLL_INTERVAL_MS = 60_000;

export default function useAcquisitionWatch(enabled: boolean, hasPaid: boolean) {
  const { scanAcquisitionThreats, dismountAllVolumes } = useBackend();
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

        // De-dup: only respond to names not already seen this session.
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
          `Possible forensic acquisition tooling detected: ${fresh.join(", ")}. Dismounting encrypted volumes.`,
          10_000,
        );

        try {
          const dismountRes = await dismountAllVolumes(true);
          if (cancelled) return;
          if (dismountRes.success) {
            recordEvidence(
              "monitor",
              "warn",
              `Auto-dismounted encrypted volumes in response to: ${fresh.join(", ")}`,
            );
          } else {
            recordEvidence(
              "monitor",
              "warn",
              `Auto-dismount failed after detecting: ${fresh.join(", ")} — ${dismountRes.error || "unknown error"}`,
            );
            showError("Detected acquisition tooling but could not dismount volumes — check manually.", 10_000);
          }
        } catch (dismountErr) {
          if (cancelled) return;
          console.warn("[useAcquisitionWatch] auto-dismount failed:", dismountErr);
          showError("Detected acquisition tooling but could not dismount volumes — check manually.", 10_000);
        }
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
  }, [enabled, hasPaid, scanAcquisitionThreats, dismountAllVolumes]);
}
