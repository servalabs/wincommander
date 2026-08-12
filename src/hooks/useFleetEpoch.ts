// src/hooks/useFleetEpoch.ts
//
// Polls the Pro sidecar for pending fleet policy epochs and successful remote
// command state handoffs. Runs only when fleet is enabled (app.fleet.enabled).
// On success it invalidates settings so every panel re-renders with current
// admin intent and machine state without a manual refresh.
//
// A wake-delivered command can arrive between heartbeats, so keep this local
// sidecar poll short. It performs no network request itself.

import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useQueryClient } from "@tanstack/react-query";
import { settingsKeys } from "./queries/useSettingsQuery";

const POLL_MS = 2_000;

export default function useFleetEpoch(fleetEnabled: boolean) {
  const qc = useQueryClient();
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const inFlightRef = useRef(false);

  useEffect(() => {
    if (!fleetEnabled) {
      if (timerRef.current) clearInterval(timerRef.current);
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

    // Run immediately on enable, then on every tick.
    void apply();
    timerRef.current = setInterval(() => void apply(), POLL_MS);

    return () => {
      if (timerRef.current) clearInterval(timerRef.current);
    };
  }, [fleetEnabled, qc]);
}
