// src/hooks/useFleetEpoch.ts
//
// Polls the Pro sidecar for pending fleet policy epochs and successful remote
// command state handoffs. Runs only when fleet is enabled (app.fleet.enabled).
// On success it invalidates settings so every panel re-renders with current
// admin intent and machine state without a manual refresh.
//
// The native process owns the recurring 60-second policy heartbeat and its
// failure backoff. The desktop only performs one prompt reconciliation when
// Fleet becomes enabled. Keeping a second 2-second browser timer here made
// every open window create a fresh, successful "policy sync" operation even
// when there was no policy to apply.

import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useQueryClient } from "@tanstack/react-query";
import { settingsKeys } from "./queries/useSettingsQuery";

export default function useFleetEpoch(fleetEnabled: boolean) {
  const qc = useQueryClient();
  const inFlightRef = useRef(false);

  useEffect(() => {
    if (!fleetEnabled) {
      return;
    }

    const apply = async () => {
      if (inFlightRef.current) return;
      inFlightRef.current = true;
      try {
        let result: {
          applied: boolean;
          version?: number;
          remoteUpdatesApplied?: number;
        } | null = null;
        try {
          result = await invoke<{
            applied: boolean;
            version?: number;
            remoteUpdatesApplied?: number;
          }>("fleet_apply_pending_epoch");
        } catch {
          // The background Rust retry loop owns fleet transport escalation.
        }
        // Snapshot after applying policy/command state so the Fleet server's
        // next check-in cannot observe the just-replaced, stale settings.
        await invoke("fleet_update_posture_snapshot").catch(() => {});
        if (result?.applied) {
          // A policy epoch or verified command state was applied — force
          // every panel to re-read settings immediately.
          void qc.invalidateQueries({ queryKey: settingsKeys.all });
        }
      } finally {
        inFlightRef.current = false;
      }
    };

    // Reconcile once when Fleet is enabled (including a Fleet setting pushed
    // while this window is open). The native loop handles subsequent changes
    // without duplicating diagnostics or IPC from every desktop window.
    void apply();
  }, [fleetEnabled, qc]);
}
