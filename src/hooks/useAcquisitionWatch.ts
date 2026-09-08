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
import { newDiagnosticOperationId, recordDiagnostic } from "../lib/diagnostics";

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
      const operationId = newDiagnosticOperationId("acquisition");
      try {
        const res = await scanAcquisitionThreats();
        if (cancelled) return;
        if (!res.success || !res.data) {
          recordDiagnostic({ operationId, feature: "acquisition", action: "threat_scan", stage: "runtime",
            lifecycle: "applying", outcome: "failed", errorCode: "ACQ.SCAN.FAILED",
            severity: "warn", retryability: "automatic", suggestedNextAction: "retry",
            privacyClass: "local_sensitive", context: { state: "scan_failed" } });
          return;
        }
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

        recordDiagnostic({ operationId, feature: "acquisition", action: "threat_detection", stage: "monitor",
          lifecycle: "verified", outcome: "succeeded", severity: "critical", retryability: "never",
          suggestedNextAction: "review_status", privacyClass: "local_sensitive",
          context: { reason_category: "acquisition_tool_detected", state: "detected" } });

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
            recordDiagnostic({ operationId, feature: "vault", action: "acquisition_auto_dismount", stage: "cleanup",
              lifecycle: "applied", outcome: "succeeded", severity: "warn", retryability: "never",
              suggestedNextAction: "review_status", privacyClass: "local_sensitive",
              context: { reason_category: "acquisition_tool_detected", state: "dismounted" } });
            recordEvidence(
              "monitor",
              "warn",
              `Auto-dismounted encrypted volumes in response to: ${fresh.join(", ")}`,
            );
          } else {
            recordDiagnostic({ operationId, feature: "vault", action: "acquisition_auto_dismount", stage: "cleanup",
              lifecycle: "applying", outcome: "failed", errorCode: "VLT.ACQUISITION.DISMOUNT_FAILED",
              severity: "critical", retryability: "manual", suggestedNextAction: "dismount_manually",
              privacyClass: "local_sensitive", context: { reason_category: "acquisition_tool_detected", state: "dismount_failed" } });
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
          recordDiagnostic({ operationId, feature: "vault", action: "acquisition_auto_dismount", stage: "cleanup",
            lifecycle: "applying", outcome: "failed", errorCode: "VLT.ACQUISITION.DISMOUNT_FAILED",
            severity: "critical", retryability: "manual", suggestedNextAction: "dismount_manually",
            privacyClass: "local_sensitive", context: { reason_category: "acquisition_tool_detected", state: "dismount_failed" } });
          showError("Detected acquisition tooling but could not dismount volumes — check manually.", 10_000);
        }
      } catch (err) {
        console.warn("[useAcquisitionWatch] scan failed:", err);
        recordDiagnostic({ operationId, feature: "acquisition", action: "threat_scan", stage: "runtime",
          lifecycle: "applying", outcome: "failed", errorCode: "ACQ.SCAN.FAILED",
          severity: "warn", retryability: "automatic", suggestedNextAction: "retry",
          privacyClass: "local_sensitive", context: { state: "scan_failed" } });
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
