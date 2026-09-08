// src/hooks/useLockdownWords.ts
//
// useLockdownWords — drives the F-5 system-wide keyboard-hook
// trigger off settings. Paid feature; the start command itself
// require_paid()'s on the Rust side, but we also short-circuit the
// hook here so non-paid users never invoke start.

import { useEffect, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { CoercionPhraseEntry } from "../types/settings";
import { useAuthMode } from "../context/AuthModeContext";
import { showError } from "../utils/toast";
import { newDiagnosticOperationId, recordDiagnostic } from "../lib/diagnostics";

export default function useLockdownWords(
  enabled: boolean,
  phrases: CoercionPhraseEntry[],
  hasPaid: boolean,
) {
  const { mode } = useAuthMode();
  const phraseFingerprint = useMemo(
    () => phrases.map((p) => p.hash).join("|"),
    [phrases],
  );

  // Sync registered phrases to runtime first (Rust holds an in-memory
  // copy that the hook callback consults on every keystroke).
  // Failures surface as toasts because a silent set_phrases failure
  // means the hook runs against an empty REGISTERED vec — looks like
  // "the trigger doesn't fire" with no other clue.
  useEffect(() => {
    if (!hasPaid) return;
    // Decoy mode nulls appSettings → enabled=false/phrases=[]; skip so we don't
    // register [] and stop the hook, disarming the coercion trigger mid-coercion.
    if (mode === "decoy") return;
    const operationId = newDiagnosticOperationId("lockdown");
    invoke("set_lockdown_words", { phrases }).then(() => {
      recordDiagnostic({ operationId, feature: "lockdown", action: "phrase_sync", stage: "runtime",
        lifecycle: "applied", outcome: "succeeded", severity: "info", retryability: "never",
        suggestedNextAction: "none", privacyClass: "restricted", context: { state: "registered" } });
    }).catch((err) => {
      console.error("[useLockdownWords] set_phrases failed:", err);
      recordDiagnostic({ operationId, feature: "lockdown", action: "phrase_sync", stage: "runtime",
        lifecycle: "applying", outcome: "failed", errorCode: "LCK.PHRASE.SYNC_FAILED",
        severity: "error", retryability: "manual", suggestedNextAction: "review_settings",
        privacyClass: "restricted", context: { state: "registration_failed" } });
      showError("Lockdown words could not register. Review Privacy settings and try again.");
    });
  }, [hasPaid, phraseFingerprint, phrases, mode]);

  useEffect(() => {
    if (!hasPaid) {
      invoke("stop_lockdown_words").then(() => {
        recordDiagnostic({ feature: "lockdown", action: "word_monitor", stage: "runtime",
          lifecycle: "applied", outcome: "succeeded", severity: "info", retryability: "never",
          suggestedNextAction: "none", privacyClass: "restricted", context: { state: "stopped" } });
      }).catch(() => {
        recordDiagnostic({ feature: "lockdown", action: "word_monitor", stage: "runtime",
          lifecycle: "applying", outcome: "failed", errorCode: "LCK.MONITOR.STOP_FAILED",
          severity: "warn", retryability: "automatic", suggestedNextAction: "retry",
          privacyClass: "restricted", context: { state: "stopping" } });
      });
      return;
    }
    if (mode === "decoy") return;
    if (enabled && phrases.length > 0) {
      invoke("start_lockdown_words")
        .then(() => {
          console.log("[useLockdownWords] hook started");
          recordDiagnostic({ feature: "lockdown", action: "word_monitor", stage: "runtime",
            lifecycle: "applied", outcome: "succeeded", severity: "info", retryability: "never",
            suggestedNextAction: "none", privacyClass: "restricted", context: { state: "armed" } });
        })
        .catch((err) => {
          console.error("[useLockdownWords] start failed:", err);
          recordDiagnostic({ feature: "lockdown", action: "word_monitor", stage: "runtime",
            lifecycle: "applying", outcome: "failed", errorCode: "LCK.MONITOR.START_FAILED",
            severity: "error", retryability: "manual", suggestedNextAction: "retry",
            privacyClass: "restricted", context: { state: "arming" } });
          showError("Lockdown-word monitor could not start. Review Privacy settings and try again.");
        });
    } else {
      invoke("stop_lockdown_words").then(() => {
        recordDiagnostic({ feature: "lockdown", action: "word_monitor", stage: "runtime",
          lifecycle: "applied", outcome: "succeeded", severity: "info", retryability: "never",
          suggestedNextAction: "none", privacyClass: "restricted", context: { state: "stopped" } });
      }).catch(() => {
        recordDiagnostic({ feature: "lockdown", action: "word_monitor", stage: "runtime",
          lifecycle: "applying", outcome: "failed", errorCode: "LCK.MONITOR.STOP_FAILED",
          severity: "warn", retryability: "automatic", suggestedNextAction: "retry",
          privacyClass: "restricted", context: { state: "stopping" } });
      });
    }
  }, [enabled, phrases.length, hasPaid, mode]);
}
