// src/hooks/useAutoHeal.ts
//
// When app.autoHeal is enabled, this hook detects drift (ideal ≠ current)
// after every system probe and silently re-applies the desired commands.
//
// Safeguards:
//   - Never auto-heals isAction (one-shot) or irreversible toggles
//   - Requires ideal.* to be explicitly set (undefined = user never chose)
//   - Per-toggle 60-second cooldown prevents infinite heal loops if the
//     OS keeps reverting (e.g., a policy or another app owns the setting)
//   - Only fires when current.* actually changes (post-probe), not on
//     every render, to avoid hammering the backend during normal use

import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useQueryClient } from "@tanstack/react-query";
import { useSettingsQuery, settingsKeys } from "./queries/useSettingsQuery";
import { executeBackendCommand } from "./useBackend";
import { getRadarDriftToggles } from "../registry";
import { getToggleDrift } from "../lib/toggleDrift";
import { useAuthMode } from "../context/AuthModeContext";

const HEAL_COOLDOWN_MS = 60_000;
const DRIFT_PROBE_INTERVAL_MS = 60_000;

export default function useAutoHeal() {
  const { mode: authMode } = useAuthMode();
  const { data: appSettings } = useSettingsQuery();
  const queryClient = useQueryClient();
  const healCooldown = useRef<Record<string, number>>({});
  const isHealingRef = useRef(false);
  const lastCurrentHashRef = useRef<string>("");

  useEffect(() => {
    if (authMode === "decoy" || !appSettings?.app?.firstRunComplete) return;

    const managed = appSettings.policy?.syncMode === "managed";
    const lockEnforcing = managed && (appSettings.policy?.lockedPaths?.length ?? 0) > 0;
    if (!appSettings.app.autoHeal && !lockEnforcing) return;

    let cancelled = false;
    const refreshProbe = async () => {
      try {
        const probe = await executeBackendCommand<unknown>("Get-WCSystemProbe");
        if (!probe.success || !probe.data || cancelled) return;
        const updated = await invoke("update_current_state", { probe: probe.data });
        if (!cancelled && updated) queryClient.setQueryData(settingsKeys.detail(), updated);
      } catch {
        // The next interval retries. A failed probe must never overwrite current.*.
      }
    };

    const interval = window.setInterval(() => { void refreshProbe(); }, DRIFT_PROBE_INTERVAL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [authMode, appSettings?.app?.autoHeal, appSettings?.app?.firstRunComplete,
    appSettings?.policy?.lockedPaths, appSettings?.policy?.syncMode, queryClient]);

  useEffect(() => {
    // KT: decoy guard — real hardening commands fired during a coerced decoy
    // session would be observable on the machine (registry writes, PS invokes).
    if (authMode === 'decoy') return;
    if (!appSettings?.app?.firstRunComplete) return;
    if (isHealingRef.current) return;

    const autoHealOn = !!appSettings?.app?.autoHeal;
    // Fleet Control Plane P2: a fleet-managed device must keep org-LOCKED
    // settings at their published value even if the user has auto-heal OFF —
    // the lock holds against direct-in-Windows edits. Locked paths are
    // ideal-relative dot prefixes (same convention the backend enforces in
    // patch_settings_cmd via `path.starts_with(p)`).
    const managed = appSettings?.policy?.syncMode === "managed";
    const lockedPaths = appSettings?.policy?.lockedPaths ?? [];
    const lockEnforcing = managed && lockedPaths.length > 0;
    if (!autoHealOn && !lockEnforcing) return;

    const isLocked = (t: { settingsPath?: string }) => {
      const p = (t.settingsPath ?? "").replace(/^ideal\./, "");
      return p !== "" && lockedPaths.some((lp) => p === lp || p.startsWith(`${lp}.`));
    };

    // Only react when current.* changes (a probe just ran)
    const currentHash = JSON.stringify(appSettings.current);
    if (currentHash === lastCurrentHashRef.current) return;
    lastCurrentHashRef.current = currentHash;

    const now = Date.now();

    const drifted = getRadarDriftToggles().filter(t => {
      // One-shot actions and incomplete registry rows cannot be safely replayed.
      if (t.isAction || (!t.capabilityKey && (!t.enableCmd || !t.disableCmd))) return false;
      if (!getToggleDrift(appSettings, t)) return false;
      const lastHeal = healCooldown.current[t.id] ?? 0;
      if ((now - lastHeal) <= HEAL_COOLDOWN_MS) return false;
      // When auto-heal is off, only re-assert org-locked toggles.
      return autoHealOn || isLocked(t);
    });

    if (drifted.length === 0) return;

    isHealingRef.current = true;
    drifted.forEach(t => { healCooldown.current[t.id] = now; });

    Promise.all(
      drifted.map(async t => {
        const drift = getToggleDrift(appSettings, t);
        if (!drift) return;
        try {
          if (t.capabilityKey) {
            await executeBackendCommand("Set-AppCapabilityAccess", {
              Capability: t.capabilityKey,
              Access: drift.targetChecked ? "Deny" : "Allow",
            });
          } else {
            await executeBackendCommand(drift.targetChecked ? t.enableCmd : t.disableCmd);
          }
        } catch {
          // Best-effort — silent; cooldown prevents a tight retry loop
        }
      })
    ).finally(async () => {
      isHealingRef.current = false;
      await queryClient.refetchQueries({ queryKey: settingsKeys.all });
    });
  }, [authMode, appSettings, queryClient]);
}
