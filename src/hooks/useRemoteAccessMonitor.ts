// src/hooks/useRemoteAccessMonitor.ts
//
// useRemoteAccessMonitor — drives the #4 remote-access monitor off
// settings. Same pattern as useDecoyMonitor: a single global hook reads
// `enabled` + per-tool overrides and reconciles the Pro runtime state.
// The Pro module owns the catalogue + poll task; this hook only pushes
// the desired enabled-set and starts/stops the detector.
//
// Reconciliation on every settings change:
//   1. If enabled, push each per-tool override (set_remote_access_tool_
//      enabled) so a trimmed catalogue survives restarts, then start the
//      poll task (idempotent on the Pro side).
//   2. If disabled, stop the poll task.
//
// All invokes are best-effort: the commands gate on require_paid +
// PRO_NOT_INSTALLED in the Free wrapper / sidecar, so on a Free build or
// without the Pro sidecar they reject quietly and the hook is a no-op.

import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";

export default function useRemoteAccessMonitor(
  enabled: boolean,
  toolOverrides: Record<string, boolean> | null,
) {
  const lastReconciled = useRef<string>("");

  useEffect(() => {
    let cancelled = false;
    const reconcile = async () => {
      if (cancelled) return;
      try {
        if (!enabled) {
          await invoke("stop_remote_access_monitor").catch(() => {});
          return;
        }
        // Push per-tool overrides BEFORE start so the first poll already
        // honours a trimmed catalogue. Missing key = tool stays on.
        if (toolOverrides) {
          for (const [toolId, on] of Object.entries(toolOverrides)) {
            await invoke("set_remote_access_tool_enabled", { toolId, enabled: on }).catch(
              () => {},
            );
          }
        }
        await invoke("start_remote_access_monitor").catch(() => {});
      } catch (err) {
        console.warn("[useRemoteAccessMonitor] reconcile failed:", err);
      }
    };

    // Cheap dedup: skip if nothing changed since the last reconciliation.
    const fingerprint = `${enabled}|${JSON.stringify(
      Object.entries(toolOverrides ?? {}).sort(),
    )}`;
    if (fingerprint === lastReconciled.current) return;
    lastReconciled.current = fingerprint;

    reconcile();
    return () => {
      cancelled = true;
    };
  }, [enabled, toolOverrides]);
}
