// src/hooks/useManagedPolicy.ts
//
// F9 phase-2 — managed-policy toggle locking.
//
// Fetches the GPO/MDM policy from the backend once on mount and refreshes
// every 60 s (matching ManagedPolicyBanner's cadence so a live gpupdate is
// reflected promptly).
//
// The hook is intentionally side-effect-only at the section level: callers
// obtain { managed, values } once and pass the result to isToggleLocked(),
// which is a pure helper with no per-toggle IPC.  Zero N+1 risk.
//
// ADDITIVE GUARANTEE: when no policy is present (managed === false, or the
// invoke fails), values is an empty object and isToggleLocked() always
// returns false — the toggle grid is completely unchanged.

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

// ── Types ────────────────────────────────────────────────────────────────────

export interface ManagedPolicy {
  managed: boolean;
  source: string;
  values: Record<string, boolean | string>;
}

// ── LOCK_MAP ─────────────────────────────────────────────────────────────────
//
// Maps a policy registry value name → the toggle id it locks.
// Only include entries where:
//   - a recognised policy key (LockTelemetryOff etc.) exists, AND
//   - a clean 1:1 toggle id target can be named.
//
// Extend this map as new Lock* policy keys are shipped.

export const LOCK_MAP: Record<string, string> = {
  LockTelemetryOff: "telemetry",
};

// ── Pure helper ──────────────────────────────────────────────────────────────

/**
 * Returns true iff:
 *  - some policy key in LOCK_MAP maps to `toggleId`, AND
 *  - that key is present in `values` with a truthy value.
 *
 * Passing an empty `values` object (the no-policy default) always returns false.
 */
export function isToggleLocked(
  values: Record<string, boolean | string>,
  toggleId: string,
): boolean {
  for (const [policyKey, mappedToggleId] of Object.entries(LOCK_MAP)) {
    if (mappedToggleId === toggleId && values[policyKey]) {
      return true;
    }
  }
  return false;
}

// ── Hook ─────────────────────────────────────────────────────────────────────

const POLL_INTERVAL_MS = 60_000;

const EMPTY_POLICY: ManagedPolicy = {
  managed: false,
  source: "",
  values: {},
};

export function useManagedPolicy(): ManagedPolicy {
  const [policy, setPolicy] = useState<ManagedPolicy>(EMPTY_POLICY);

  useEffect(() => {
    let cancelled = false;

    async function fetchPolicy() {
      try {
        const result = await invoke<ManagedPolicy>("get_managed_policy");
        if (!cancelled) setPolicy(result);
      } catch {
        // Best-effort: if the command fails (e.g. older build without the
        // command registered), silently stay on the empty-policy default so
        // no toggle is accidentally locked on error.
        if (!cancelled) setPolicy(EMPTY_POLICY);
      }
    }

    fetchPolicy();
    const id = setInterval(fetchPolicy, POLL_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, []);

  return policy;
}
