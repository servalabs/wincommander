import type { PackageUpdateInventory } from "../hooks/useBackend";

export interface PackageUpdateInventorySnapshot {
  status: "idle" | "checking" | "ready" | "failed";
  inventory: PackageUpdateInventory | null;
  catalogInventoryFresh: boolean;
  error: string | null;
  lastCheckedAt: string | null;
}

const initialSnapshot: PackageUpdateInventorySnapshot = {
  status: "idle",
  inventory: null,
  catalogInventoryFresh: false,
  error: null,
  lastCheckedAt: null,
};

let snapshot = initialSnapshot;
let inFlight: Promise<PackageUpdateInventory> | null = null;
const listeners = new Set<() => void>();

function publish(next: PackageUpdateInventorySnapshot): void {
  snapshot = next;
  listeners.forEach((listener) => listener());
}

export function getPackageUpdateInventorySnapshot(): PackageUpdateInventorySnapshot {
  return snapshot;
}

export function subscribeToPackageUpdateInventory(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function setPackageUpdateCatalogInventoryFresh(fresh: boolean): void {
  if (snapshot.catalogInventoryFresh === fresh) return;
  publish({ ...snapshot, catalogInventoryFresh: fresh });
}

/**
 * Share startup, panel, and post-update scans so only one set of package
 * managers is queried at a time. The most recent successful inventory remains
 * visible while a refresh is in progress or after a later refresh fails.
 */
export function runPackageUpdateInventoryCheck(
  check: () => Promise<PackageUpdateInventory>,
): Promise<PackageUpdateInventory> {
  if (inFlight) return inFlight;

  publish({ ...snapshot, status: "checking", error: null });
  const task = Promise.resolve()
    .then(check)
    .then((inventory) => {
      publish({
        status: "ready",
        inventory,
        catalogInventoryFresh: snapshot.catalogInventoryFresh,
        error: null,
        lastCheckedAt: new Date().toISOString(),
      });
      return inventory;
    })
    .catch((cause: unknown) => {
      const error = cause instanceof Error ? cause.message : String(cause);
      publish({ ...snapshot, status: "failed", error });
      throw cause;
    })
    .finally(() => {
      if (inFlight === task) inFlight = null;
    });
  inFlight = task;
  return task;
}
