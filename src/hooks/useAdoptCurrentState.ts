// src/hooks/useAdoptCurrentState.ts
//
// After every system probe, if current.* is true (a setting is applied in
// Windows) but ideal.* has never been set (null / undefined — the user never
// explicitly toggled it in Commander), write ideal.* = true to adopt the
// current Windows state as the stored preference.
//
// This prevents toggles from showing OFF when the setting is actually working,
// and avoids spurious DRIFT badges on first-detection of an applied setting.
//
// Safeguards:
//   - Only fires when current.* changes (post-probe), not on every render
//   - Only adopts when ideal.* is null or undefined — never overwrites an
//     explicit user choice of false (true = already correct, false = user
//     wants it off, null/undefined = no preference yet → adopt)
//   - Skips irreversible, capability-key, and action toggles
//   - Requires firstRunComplete so it does not run while setup is unresolved

import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useQueryClient } from "@tanstack/react-query";
import { useSettingsQuery, settingsKeys } from "./queries/useSettingsQuery";
import { getByPath } from "../types/toggles";
import { getRadarDriftToggles } from "../registry";
import { useAuthMode } from "../context/AuthModeContext";

function buildDeepPatch(paths: string[], value: unknown): Record<string, unknown> {
  const result: Record<string, unknown> = {};
  for (const path of paths) {
    const parts = path.split(".");
    let cur = result as Record<string, unknown>;
    for (let i = 0; i < parts.length - 1; i++) {
      if (!cur[parts[i]] || typeof cur[parts[i]] !== "object") {
        cur[parts[i]] = {};
      }
      cur = cur[parts[i]] as Record<string, unknown>;
    }
    cur[parts[parts.length - 1]] = value;
  }
  return result;
}

export default function useAdoptCurrentState() {
  const { mode: authMode } = useAuthMode();
  const { data: appSettings } = useSettingsQuery();
  const queryClient = useQueryClient();
  const lastCurrentHashRef = useRef<string>("");

  useEffect(() => {
    // KT: decoy guard — adopting real probe data into ideal.* during a coerced
    // session would write real config back to disk, detectable by a forensic examiner.
    if (authMode === 'decoy') return;
    if (!appSettings?.app?.firstRunComplete) return;

    const currentHash = JSON.stringify(appSettings.current);
    if (currentHash === lastCurrentHashRef.current) return;
    lastCurrentHashRef.current = currentHash;

    const toAdopt: string[] = [];

    for (const toggle of getRadarDriftToggles()) {
      // Capability-key toggles use string "Deny"/"Allow" semantics — skip
      if ((toggle as any).capabilityKey) continue;

      const idealRaw = getByPath(appSettings, toggle.settingsPath);
      // Only adopt when there is no stored preference at all
      if (idealRaw !== null && idealRaw !== undefined) continue;

      const currentRaw = getByPath(appSettings, toggle.currentPath);
      // Only adopt when the probe confirmed the setting is applied (boolean true)
      if (currentRaw !== true) continue;

      toAdopt.push(toggle.settingsPath);
    }

    if (toAdopt.length === 0) return;

    const patch = buildDeepPatch(toAdopt, true);

    invoke<unknown>("patch_settings_cmd", { patch })
      .then((updated) => {
        if (updated) queryClient.setQueryData(settingsKeys.detail(), updated);
      })
      .catch(() => { /* best-effort; probe will re-run shortly */ });
  }, [authMode, appSettings, queryClient]);
}
