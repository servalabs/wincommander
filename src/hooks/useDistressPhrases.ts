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
    invoke("set_distress_phrases", { phrases }).catch((err) => {
      console.error("[useDistressPhrases] sync failed:", err);
      showError(`Distress phrases: failed to sync — ${err}`);
    });
  }, [hasPaid, fingerprint, phrases, mode]);
}
