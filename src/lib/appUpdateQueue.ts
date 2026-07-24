// src/lib/appUpdateQueue.ts
//
// Tracks which app (winget) ids are currently CLAIMED by an in-flight upgrade —
// queued by "Update All Apps", "Update Selected", a per-row update, or the
// dashboard's "Fix Everything". Every app-update queuing site marks its ids
// here SYNCHRONOUSLY before any await, so the dashboard's Needs Attention /
// Fix All can drop those items the instant they're queued and never list (or
// re-run) them a second time — closing the window between "clicked Update All"
// and the update task actually registering as running.
//
// Deliberately module-level (not persisted): an in-flight upgrade doesn't
// survive a reload, so the claim shouldn't either. Mirrors the subscribe/
// snapshot shape of notificationStore.ts.

const queued = new Set<string>();

/** Fired on any mutation so subscribers re-read. */
export const APP_UPDATE_QUEUE_CHANGED_EVENT = "wc-app-update-queue-changed";

function emit(): void {
  window.dispatchEvent(new Event(APP_UPDATE_QUEUE_CHANGED_EVENT));
}

/** Mark app ids as claimed by an in-flight upgrade. Blank ids are ignored. */
export function markAppUpdatesQueued(ids: Iterable<string>): void {
  let changed = false;
  for (const id of ids) {
    const key = (id ?? "").trim();
    if (key && !queued.has(key)) {
      queued.add(key);
      changed = true;
    }
  }
  if (changed) emit();
}

/**
 * Claim only the ids NOT already owned by another in-flight upgrade, and return
 * exactly the ids this caller newly claimed — so it can release precisely those
 * in its finally() without clobbering another op's claim (a per-row update and
 * "Update All" can run at once).
 */
export function claimFreeAppUpdates(ids: Iterable<string>): string[] {
  const claimed: string[] = [];
  for (const id of ids) {
    const key = (id ?? "").trim();
    if (key && !queued.has(key)) {
      queued.add(key);
      claimed.push(key);
    }
  }
  if (claimed.length) emit();
  return claimed;
}

/** Release app ids once their upgrade settles (success or failure). */
export function clearAppUpdatesQueued(ids: Iterable<string>): void {
  let changed = false;
  for (const id of ids) {
    const key = (id ?? "").trim();
    if (key && queued.delete(key)) changed = true;
  }
  if (changed) emit();
}

/** True if this app id is currently claimed by an in-flight upgrade. */
export function isAppUpdateQueued(id: string): boolean {
  return queued.has((id ?? "").trim());
}

/** Snapshot of all currently-claimed app ids. */
export function getQueuedAppUpdateIds(): Set<string> {
  return new Set(queued);
}

/** Subscribe to queue changes. Returns an unsubscribe fn. */
export function subscribeAppUpdateQueue(cb: () => void): () => void {
  window.addEventListener(APP_UPDATE_QUEUE_CHANGED_EVENT, cb);
  return () => window.removeEventListener(APP_UPDATE_QUEUE_CHANGED_EVENT, cb);
}
