// src/hooks/updaterStore.ts
//
// Frontend mirror of the Rust self-update scheduler (src-tauri/.../updater.rs).
//
// The Rust side OWNS the check/download cadence (every 7 days, ~5min adaptive
// retry while offline) and runs regardless of window state. It also checks
// once ~30s after every fresh process launch (INITIAL_DELAY), independent of
// the 7-day repeat interval, which only governs re-checks within one
// long-running session. The frontend no longer runs its own setInterval — it
// just listens for `updater://state` events and reflects the emitted phase.
// A module-level store (shared via
// useSyncExternalStore) lets both the Dashboard banner (UpdaterStatus) and
// the combined UpdateFlowDialog / useUpdateFlow read one snapshot without
// prop drilling.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useSyncExternalStore } from "react";
import { executeBackendCommand } from "./useBackend";

export type UpdaterPhase =
  | "idle"
  | "checking"
  | "available"
  | "downloading"
  | "staged"
  | "ready"
  | "error";

export interface UpdaterState {
  phase: UpdaterPhase;
  version: string | null;
  currentVersion: string | null;
  body: string | null;
  error: string | null;
}

// Wire payload from Rust (`StatePayload` in updater.rs, snake_case).
interface UpdaterEventPayload {
  phase: UpdaterPhase;
  version?: string | null;
  current_version?: string | null;
  body?: string | null;
  error?: string | null;
}

let state: UpdaterState = {
  phase: "idle",
  version: null,
  currentVersion: null,
  body: null,
  error: null,
};

const subscribers = new Set<() => void>();
let started = false;

function notify() {
  subscribers.forEach((s) => s());
}

function subscribe(cb: () => void): () => void {
  subscribers.add(cb);
  return () => {
    subscribers.delete(cb);
  };
}

function getSnapshot(): UpdaterState {
  return state;
}

function applyPayload(p: UpdaterEventPayload) {
  state = {
    phase: p.phase,
    version: p.version ?? null,
    currentVersion: p.current_version ?? null,
    body: p.body ?? null,
    error: p.error ?? null,
  };
  notify();
}

/** Attach the backend event listener. Idempotent — call once at app root.
 *  Never unlistens (app-lifetime); the guard prevents a duplicate listener. */
export function startUpdaterListener(): void {
  if (started) return;
  started = true;
  // Hydrate immediately from the last state the Rust scheduler emitted. A
  // frontend that mounts LATE (behind the calculator/startup-PIN gate) would
  // otherwise miss the fire-and-forget event fired ~30s after launch and sit on
  // stale "idle" until the next 7-day cycle. Guarded on the initial "idle" so
  // this backfill never clobbers a live event that already arrived.
  void invoke<UpdaterEventPayload | null>("updater_current_state")
    .then((p) => {
      if (p && state.phase === "idle") applyPayload(p);
    })
    .catch(() => {});
  void (async () => {
    try {
      await listen<UpdaterEventPayload>("updater://state", (event) => {
        applyPayload(event.payload);
        // Flush DNS cache silently after each successful check cycle so the
        // update server lookup doesn't persist in the local resolver cache.
        const phase = event.payload.phase;
        if (phase === "idle" || phase === "available" || phase === "staged") {
          executeBackendCommand("Clear-DnsCache").catch(() => {});
        }
      });
    } catch (err) {
      console.error("[updater] failed to attach listener:", err);
      started = false; // allow a later retry on next mount
    }
  })();
}

/** React hook: subscribe a component to the shared updater snapshot. */
export function useUpdater(): UpdaterState {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}
