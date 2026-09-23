import { describe, expect, test } from "bun:test";
import type { ManagerInventory } from "../../hooks/useBackend";
import type { AppInventorySnapshot } from "../../types/settings";
import { collectManagerUpdates, filterCatalogDuplicates, managerUpdateStatus, refreshPackageAndAppInventories } from "./packageUpdateDisplay";

const manager = (name: string, packages: string[]): ManagerInventory => ({
  manager: name,
  available: true,
  error: null,
  updates: packages.map((pkg, index) => ({
    id: `${name}-${index}`,
    manager: name,
    package: pkg,
    currentVersion: "1.0",
    availableVersion: "2.0",
  })),
});

describe("package update display", () => {
  test("hides catalog and other-package duplicates only from Winget", () => {
    const inventory = {
      manifestApps: [{ id: "Microsoft.PowerShell", updateAvailable: true }],
      pendingUpdates: [{ id: "Git.Git" }],
    } as Pick<AppInventorySnapshot, "manifestApps" | "pendingUpdates">;

    const filtered = filterCatalogDuplicates([
      manager("winget", [" microsoft.powershell ", "Git.Git", "Microsoft.WindowsTerminal"]),
      manager("chocolatey", ["git", "powershell-core"]),
    ], inventory);

    expect(filtered[0].updates.map((update) => update.package)).toEqual(["Microsoft.WindowsTerminal"]);
    expect(filtered[1].updates.map((update) => update.package)).toEqual(["git", "powershell-core"]);
  });

  test("keeps every manager result when the app inventory has not loaded", () => {
    const managers = [manager("winget", ["Git.Git"]), manager("npm", ["@scope/package"])];
    expect(filterCatalogDuplicates(managers, null)).toBe(managers);
  });

  test("flattens manager updates, excludes catalog duplicates, and keeps each source manager", () => {
    const inventory = {
      manifestApps: [{ id: "Microsoft.PowerShell", updateAvailable: true }],
      pendingUpdates: [{ id: "Git.Git" }],
    } as Pick<AppInventorySnapshot, "manifestApps" | "pendingUpdates">;
    const rows = collectManagerUpdates([
      manager("winget", ["microsoft.powershell", "Git.Git", "Microsoft.WindowsTerminal"]),
      manager("chocolatey", ["git", "powershell-core"]),
      manager("scoop", ["7zip"]),
    ], inventory);

    expect(rows.map(({ manager: source, update }) => ({ manager: source.manager, package: update.package }))).toEqual([
      { manager: "winget", package: "Microsoft.WindowsTerminal" },
      { manager: "chocolatey", package: "git" },
      { manager: "chocolatey", package: "powershell-core" },
      { manager: "scoop", package: "7zip" },
    ]);
  });

  test("checks the app inventory before the package manager inventory in one refresh flow", async () => {
    const calls: string[] = [];
    const result = { managers: [], cancelled: false };
    const refreshed = await refreshPackageAndAppInventories(
      async () => { calls.push("apps"); },
      async () => { calls.push("package managers"); return result; },
    );

    expect(calls).toEqual(["apps", "package managers"]);
    expect(refreshed).toBe(result);
  });

  test("gives a clear status for each package manager", () => {
    expect(managerUpdateStatus(manager("winget", ["Git.Git"]))).toEqual({
      label: "1 update available",
      tone: "accent",
    });
    expect(managerUpdateStatus(manager("winget", [])).label).toBe("No updates");
    expect(managerUpdateStatus({ ...manager("winget", []), available: false }).label).toBe("Unavailable");
    expect(managerUpdateStatus({ ...manager("winget", []), error: "scan failed" }).label).toBe("Check failed");
  });
});
