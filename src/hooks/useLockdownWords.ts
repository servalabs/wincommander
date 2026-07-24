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
    invoke("set_lockdown_words", { phrases }).catch((err) => {
      console.error("[useLockdownWords] set_phrases failed:", err);
      showError(`Lockdown words: failed to register phrases — ${err}`);
    });
  }, [hasPaid, phraseFingerprint, phrases, mode]);

  useEffect(() => {
    if (!hasPaid) {
      invoke("stop_lockdown_words").catch(() => {});
      return;
    }
    if (mode === "decoy") return;
    if (enabled && phrases.length > 0) {
      invoke("start_lockdown_words")
        .then(() => {
          console.log("[useLockdownWords] hook started");
        })
        .catch((err) => {
          console.error("[useLockdownWords] start failed:", err);
          showError(`Lockdown words: hook failed to start — ${err}`);
        });
    } else {
      invoke("stop_lockdown_words").catch(() => {});
    }
  }, [enabled, phrases.length, hasPaid, mode]);
}
