// src/hooks/useDistressPhrases.ts
//
// Syncs privacy.distressPhrases from settings into the Rust
// DISTRESS_REGISTERED in-memory list on every settings change.
// Mirrors useLockdownWords (F-5) exactly — see that file for context.

import { useEffect, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AppSettings } from "../types/settings";
import { useAuthMode } from "../context/AuthModeContext";
import { showError } from "../utils/toast";
import { newDiagnosticOperationId, recordDiagnostic } from "../lib/diagnostics";

export default function useDistressPhrases(
  appSettings: AppSettings | null,
  hasPaid: boolean,
) {
  const { mode } = useAuthMode();
  const rawPhrases = appSettings?.ideal?.privacy?.distressPhrases;
  const phrases = useMemo(
    () => rawPhrases ?? [],
    [rawPhrases],
  );

  const fingerprint = useMemo(
    () => phrases.map((p) => p.hash).join("|"),
    [phrases],
  );

  useEffect(() => {
    if (!hasPaid) return;
    // In decoy mode appSettings is null, so `phrases` collapses to []. Pushing
    // that would wipe DISTRESS_REGISTERED and disarm the distress phrases mid-
    // coercion. Skip the sync so the phrases armed before the switch stay live.
    if (mode === "decoy") return;
    const operationId = newDiagnosticOperationId("privacy");
    invoke("set_distress_phrases", { phrases }).then(() => {
      recordDiagnostic({ operationId, feature: "privacy", action: "distress_phrase_sync", stage: "runtime",
        lifecycle: "applied", outcome: "succeeded", severity: "info", retryability: "never",
        suggestedNextAction: "none", privacyClass: "restricted", context: { state: "registered" } });
    }).catch((err) => {
      console.error("[useDistressPhrases] sync failed:", err);
      recordDiagnostic({ operationId, feature: "privacy", action: "distress_phrase_sync", stage: "runtime",
        lifecycle: "applying", outcome: "failed", errorCode: "PRV.DISTRESS.SYNC_FAILED",
        severity: "error", retryability: "manual", suggestedNextAction: "review_settings",
        privacyClass: "restricted", context: { state: "registration_failed" } });
      showError("Distress phrases could not sync. Review Privacy settings and try again.");
    });
  }, [hasPaid, fingerprint, phrases, mode]);
}
