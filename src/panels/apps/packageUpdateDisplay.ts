import type { ManagerInventory } from "../../hooks/useBackend";
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
