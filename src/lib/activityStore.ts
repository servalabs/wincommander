// Tracks in-flight backend work so the idle-pause timer can defer engagement
// while a task (app install, applying a toggle, self-destruct, …) is running.
// External store pattern so any component can subscribe via useSyncExternalStore
// without prop-drilling or lifting state.

import { useSyncExternalStore } from "react";

let pending = 0;
const listeners = new Set<() => void>();

function notify() {
  listeners.forEach((fn) => fn());
}

function subscribe(fn: () => void) {
  listeners.add(fn);
  return () => {
    listeners.delete(fn);
  };
}

function getSnapshot() {
  return pending;
}

// Wrap any backend promise so the in-flight count tracks it. Counter is
// decremented in finally so a rejection still unwinds correctly.
export async function trackBackendWork<T>(work: Promise<T>): Promise<T> {
  pending += 1;
  notify();
  try {
    return await work;
  } finally {
    pending -= 1;
    notify();
  }
}

export function useHasActiveBackendWork(): boolean {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot) > 0;
}
