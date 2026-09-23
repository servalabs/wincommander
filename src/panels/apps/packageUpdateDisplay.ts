import type { ManagerInventory, PackageUpdateInventory } from "../../hooks/useBackend";
import type { AppInventorySnapshot } from "../../types/settings";

type CatalogInventory = Pick<AppInventorySnapshot, "manifestApps" | "pendingUpdates"> | null;

function packageIdentity(value: string): string {
  return value.trim().toLocaleLowerCase();
}

/**
 * The app catalog already gives Winget updates their own actionable cards.
 * Keep the manager scanner for updates outside that inventory, rather than
 * presenting the same Winget package twice with two competing actions.
 */
export function filterCatalogDuplicates(
  managers: ManagerInventory[],
  inventory: CatalogInventory,
): ManagerInventory[] {
  if (!inventory) return managers;

  const catalogIds = new Set<string>();
  inventory.manifestApps.forEach((app) => {
    if (app.updateAvailable) catalogIds.add(packageIdentity(app.id));
  });
  inventory.pendingUpdates.forEach((update) => {
    if (update.id) catalogIds.add(packageIdentity(update.id));
  });

  if (!catalogIds.size) return managers;

  return managers.map((manager) => {
    if (manager.manager.toLocaleLowerCase() !== "winget") return manager;
    return {
      ...manager,
      updates: manager.updates.filter((update) => !catalogIds.has(packageIdentity(update.package))),
    };
  });
}

/**
 * Place every manager's remaining updates in one shared list. The manager id
 * stays with each row so the UI can identify its source without giving one
 * package manager a separate display or apply flow.
 */
export function collectManagerUpdates(
  managers: ManagerInventory[],
  inventory: CatalogInventory,
): Array<{ manager: ManagerInventory; update: ManagerInventory["updates"][number] }> {
  return filterCatalogDuplicates(managers, inventory).flatMap((manager) =>
    manager.updates.map((update) => ({ manager, update })),
  );
}

/**
 * Refresh the app catalog before checking the additional package managers.
 * Both sources are shown together, and running their Winget probes in parallel
 * can make the package manager contend with itself on Windows.
 */
export async function refreshPackageAndAppInventories(
  refreshAppInventory: () => Promise<void>,
  checkPackageManagers: () => Promise<PackageUpdateInventory>,
): Promise<PackageUpdateInventory> {
  await refreshAppInventory();
  return checkPackageManagers();
}

export function managerUpdateStatus(manager: ManagerInventory): {
  label: string;
  tone: "accent" | "neutral" | "warning";
} {
  if (!manager.available) return { label: "Unavailable", tone: "warning" };
  if (manager.error) return { label: "Check failed", tone: "warning" };
  if (manager.updates.length === 0) return { label: "No updates", tone: "neutral" };
  return {
    label: `${manager.updates.length} update${manager.updates.length === 1 ? "" : "s"} available`,
    tone: "accent",
  };
}
