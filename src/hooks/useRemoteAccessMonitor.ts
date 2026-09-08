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
import type { StartupProtectionOperation } from "../lib/startupProtectionReadiness";
import { newDiagnosticOperationId, recordDiagnostic } from "../lib/diagnostics";

export default function useRemoteAccessMonitor(
  enabled: boolean,
  toolOverrides: Record<string, boolean> | null,
  onStartupRearm?: (operation: StartupProtectionOperation, succeeded: boolean) => void,
) {
  const lastReconciled = useRef<string>("");

  useEffect(() => {
    let cancelled = false;
    let retryTimer: ReturnType<typeof setTimeout> | null = null;
    let attempt = 0;
    const operationId = newDiagnosticOperationId("remote_access");
    const startedAt = Date.now();
    const fingerprint = `${enabled}|${JSON.stringify(
      Object.entries(toolOverrides ?? {}).sort(),
    )}`;
    const reconcile = async () => {
      if (cancelled) return;
      try {
        if (!enabled) {
          recordDiagnostic({ operationId, feature: "remote_access", action: "stop", stage: "sidecar", lifecycle: "applying", outcome: "started", severity: "info", retryability: "never", suggestedNextAction: "none", durationMs: Date.now() - startedAt, privacyClass: "restricted", context: { state: "disabled", retry_count: attempt } });
          await invoke("stop_remote_access_monitor");
          recordDiagnostic({ operationId, feature: "remote_access", action: "stop", stage: "sidecar", lifecycle: "applied", outcome: "succeeded", severity: "info", retryability: "never", suggestedNextAction: "none", durationMs: Date.now() - startedAt, privacyClass: "restricted", context: { state: "disabled", retry_count: attempt } });
          lastReconciled.current = fingerprint;
          return;
        }
        // Push per-tool overrides BEFORE start so the first poll already
        // honours a trimmed catalogue. Missing key = tool stays on.
        if (toolOverrides) {
          for (const [toolId, on] of Object.entries(toolOverrides)) {
            await invoke("set_remote_access_tool_enabled", { toolId, enabled: on });
          }
        }
        recordDiagnostic({ operationId, feature: "remote_access", action: "start", stage: "sidecar", lifecycle: "applying", outcome: "started", severity: "info", retryability: "never", suggestedNextAction: "none", durationMs: Date.now() - startedAt, privacyClass: "restricted", context: { state: "enabled", retry_count: attempt } });
        await invoke("start_remote_access_monitor");
        if (cancelled) return;
        lastReconciled.current = fingerprint;
        recordDiagnostic({ operationId, feature: "remote_access", action: "start", stage: "sidecar", lifecycle: "applied", outcome: "succeeded", severity: "info", retryability: "never", suggestedNextAction: "none", durationMs: Date.now() - startedAt, privacyClass: "restricted", context: { state: "enabled", retry_count: attempt } });
        onStartupRearm?.("remote-access-monitor", true);
      } catch (err) {
        console.warn("[useRemoteAccessMonitor] reconcile failed:", err);
        recordDiagnostic({ operationId, feature: "remote_access", action: enabled ? "start" : "stop", stage: "sidecar", lifecycle: "applied", outcome: "failed", errorCode: enabled ? "REMOTE.ACCESS.START_FAILED" : "REMOTE.ACCESS.STOP_FAILED", severity: "warn", retryability: "automatic", suggestedNextAction: "retry", durationMs: Date.now() - startedAt, privacyClass: "restricted", context: { state: enabled ? "enabled" : "disabled", retry_count: attempt } });
        attempt += 1;
        if (!cancelled && attempt < 3) {
          retryTimer = setTimeout(() => { void reconcile(); }, attempt * 5_000);
        } else if (!cancelled && enabled) {
          recordDiagnostic({ operationId, feature: "remote_access", action: "start", stage: "sidecar", lifecycle: "applied", outcome: "degraded", errorCode: "REMOTE.ACCESS.STARTUP_DEGRADED", severity: "warn", retryability: "manual", suggestedNextAction: "retry", durationMs: Date.now() - startedAt, privacyClass: "restricted", context: { state: "enabled", retry_count: attempt } });
          onStartupRearm?.("remote-access-monitor", false);
        }
      }
    };

    // Skip only a previously successful reconciliation. A sidecar startup
    // race must not make an ON preference silently inert until settings move.
    if (fingerprint === lastReconciled.current) return;

    void reconcile();
    return () => {
      cancelled = true;
      if (retryTimer) clearTimeout(retryTimer);
    };
  }, [enabled, toolOverrides, onStartupRearm]);
}
