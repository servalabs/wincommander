// advisorPullStore.ts
//
// Module-level store for model-pull state.
//
// WHY: useAdvisor lives inside AdvisorPanel, which unmounts when the user
// switches panels. If a Pull-OllamaModel download is in progress, the
// hook's local state and its Tauri event listener are destroyed — so the
// progress bar disappears and events from the still-running backend are
// silently dropped. On remount the hook sees pulling=false and shows the
// download button at 0%.
//
// This store moves pulling + pullProgress out of the hook and into a
// module-level singleton whose Tauri listener is registered once and never
// torn down. Components subscribe on mount and unsubscribe on unmount;
// getSnapshot() always returns the live state so a remounting panel sees
// the current progress immediately.

import { listen } from "@tauri-apps/api/event";

export interface PullProgress {
  model: string;
  status: string;
  completed: number | null;
  total: number | null;
}

interface PullState {
  pulling: boolean;
  pullProgress: PullProgress | null;
}

let state: PullState = { pulling: false, pullProgress: null };
let listenerReady = false;
const subscribers = new Set<() => void>();

function notify(): void {
  subscribers.forEach((fn) => fn());
}

export function getSnapshot(): PullState {
  return state;
}

export function subscribe(fn: () => void): () => void {
  subscribers.add(fn);
  return () => {
    subscribers.delete(fn);
  };
}

export function setPulling(val: boolean): void {
  state = { ...state, pulling: val };
  notify();
}

export function setPullProgress(val: PullProgress | null): void {
  state = { ...state, pullProgress: val };
  notify();
}

/**
 * Register the global Tauri event listener for pull progress.
 * Idempotent — safe to call on every hook/component mount; the listener
 * is only registered once for the lifetime of the app.
 */
export function initPullListener(): void {
  if (listenerReady) return;
  listenerReady = true;
  void listen<PullProgress>("llm-pull-progress", (event) => {
    setPullProgress(event.payload);
  });
}
