// src/hooks/useDecoyMonitor.ts
//
// useDecoyMonitor — drives the F-2 filesystem honeypot watcher off
// settings. Same pattern as usePasteMonitor: a single global hook
// reads `enabled` + `enrolledPaths` and reconciles the Rust runtime
// state. Watcher start/stop is idempotent on the Rust side; same with
// enroll_decoy / remove_decoy.
//
// Reconciliation logic on every settings change:
//   1. If enabled, ensure watcher is running.
//   2. Diff settings paths against what Rust currently watches via
//      `list_decoys`; enroll missing, remove orphaned. Idempotent
//      so a duplicate enroll for an already-enrolled path is harmless.
//   3. If disabled, stop watcher.

import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAuthMode } from "../context/AuthModeContext";

interface DecoyInfoFromRust {
  path: string;
  exists: boolean;
}

export default function useDecoyMonitor(
  enabled: boolean,
  enrolledPaths: string[],
  readAuditEnabled: boolean,
) {
  const { mode } = useAuthMode();
  const lastReconciled = useRef<string>("");

  useEffect(() => {
    // Decoy mode nulls appSettings → enabled=false, which would stop the
    // honeypot watcher exactly when a coerced session is most at risk. Leave
    // the watcher in its last-armed state; reconciliation resumes on exit.
    if (mode === "decoy") return;
    let cancelled = false;
    const reconcile = async () => {
      if (cancelled) return;
      try {
        if (!enabled) {
          await invoke("stop_decoy_monitor");
          return;
        }
        await invoke("start_decoy_monitor");
        // Reconcile the enrolled set against Rust's current view.
        const current = await invoke<DecoyInfoFromRust[]>("list_decoys");
        const currentSet = new Set(current.map((d) => d.path));
        const targetSet = new Set(enrolledPaths);
        // Add missing
        for (const p of enrolledPaths) {
          if (!currentSet.has(p)) {
            await invoke("enroll_decoy", { path: p }).catch(() => {});
          }
        }
        // Remove orphaned (in Rust but not in settings)
        for (const p of currentSet) {
          if (!targetSet.has(p)) {
            await invoke("remove_decoy", { path: p });
          }
        }
        // Rules can only be installed after the files have been enrolled.
        // Doing this first left a persisted read-audit setting blocking the
        // entire reconciliation after an app restart.
        await invoke("set_decoy_read_audit_enabled", { enabled: readAuditEnabled });
      } catch (err) {
        console.warn("[useDecoyMonitor] reconcile failed:", err);
      }
    };

    // Skip if nothing changed since last reconciliation. Cheap dedup
    // for re-renders that don't change the actual values.
    const fingerprint = `${enabled}|${readAuditEnabled}|${[...enrolledPaths].sort().join("\n")}`;
    if (fingerprint === lastReconciled.current) return;
    lastReconciled.current = fingerprint;

    reconcile();
    return () => { cancelled = true; };
  }, [enabled, enrolledPaths, readAuditEnabled, mode]);
}
