import type { ManagerInventory, PackageUpdateInventory } from "../../hooks/useBackend";
import type { AppInventorySnapshot } from "../../types/settings";

type CatalogInventory = Pick<AppInventorySnapshot, "manifestApps" | "pendingUpdates"> | null;

export interface UnifiedPackageUpdateRow {
  key: string;
  kind: "catalog" | "manager";
  /** The package id for catalog upgrades or opaque backend id for manager updates. */
  actionId: string;
  manager: string;
  packageName: string;
  currentVersion: string;
  availableVersion: string;
}

function packageIdentity(value: string): string {
  return value.trim().toLocaleLowerCase();
}

/**
 * The app catalog owns the display and upgrade action for packages it found.
 * Suppress matching Winget scanner rows so the unified list contains one
 * actionable row per package.
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
 * Keep inventory-discovered Winget updates and the other manager results in
 * one actionable list. Winget results already represented by the app
 * inventory are removed by `collectManagerUpdates`, so each package appears
 * once while Chocolatey, Scoop, and npm rows remain alongside it.
 */
export function collectUnifiedPackageUpdates(
  managers: ManagerInventory[],
  inventory: CatalogInventory,
): UnifiedPackageUpdateRow[] {
  const catalogRows: UnifiedPackageUpdateRow[] = [];
  const seenCatalogIds = new Set<string>();
  const manifestById = new Map(
    (inventory?.manifestApps ?? []).map((app) => [packageIdentity(app.id), app]),
  );

  for (const update of inventory?.pendingUpdates ?? []) {
    const id = update.id.trim();
    const identity = packageIdentity(id);
    if (!id || seenCatalogIds.has(identity)) continue;
    seenCatalogIds.add(identity);
    const app = manifestById.get(identity);
    catalogRows.push({
      key: `catalog:${identity}`,
      kind: "catalog",
      actionId: id,
      manager: update.source?.trim() || "winget",
      packageName: update.name?.trim() || app?.name || id,
      currentVersion: update.installedVersion?.trim() || app?.installedVersion || "Unknown",
      availableVersion: update.latestVersion?.trim() || app?.latestVersion || "Unknown",
    });
  }

  for (const app of inventory?.manifestApps ?? []) {
    const id = app.id.trim();
    const identity = packageIdentity(id);
    if (!id || !app.updateAvailable || seenCatalogIds.has(identity)) continue;
    seenCatalogIds.add(identity);
    catalogRows.push({
      key: `catalog:${identity}`,
      kind: "catalog",
      actionId: id,
      manager: "winget",
      packageName: app.name?.trim() || id,
      currentVersion: app.installedVersion?.trim() || "Unknown",
      availableVersion: app.latestVersion?.trim() || "Unknown",
    });
  }

  const managerRows = collectManagerUpdates(managers, inventory).map(({ manager, update }) => ({
    key: `manager:${manager.manager.toLocaleLowerCase()}:${update.id}`,
    kind: "manager" as const,
    actionId: update.id,
    manager: manager.manager,
    packageName: update.package,
    currentVersion: update.currentVersion || "Unknown",
    availableVersion: update.availableVersion || "Unknown",
  }));

  return [...catalogRows, ...managerRows];
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
