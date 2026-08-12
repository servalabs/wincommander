import { describe, expect, test } from "bun:test";
import type { ManagerInventory } from "../../hooks/useBackend";
import type { AppInventorySnapshot } from "../../types/settings";
import { filterCatalogDuplicates } from "./packageUpdateDisplay";

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
});
