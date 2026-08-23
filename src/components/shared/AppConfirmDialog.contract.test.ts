import { describe, expect, test } from "bun:test";
import { readdirSync, readFileSync } from "node:fs";

const root = decodeURIComponent(new URL("../../../", import.meta.url).pathname).replace(/^\/([A-Za-z]:)/, "$1");

function productionSourceFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = `${dir}/${entry.name}`;
    if (entry.isDirectory()) return productionSourceFiles(path);
    if (!/\.(ts|tsx)$/.test(entry.name) || /\.test\.(ts|tsx)$/.test(entry.name)) return [];
    return [path];
  });
}
const guardedActionCallers = [
  "src/panels/privacy/UsbDevicesSection.tsx",
  "src/components/tweaks/managers/DiskCleanupGranular.tsx",
  "src/components/tweaks/managers/ScheduledTasksManager.tsx",
  "src/panels/privacy/DecoyMonitorSection.tsx",
  "src/panels/network/WifiGuardSection.tsx",
  "src/panels/apps/DebloatPanel.tsx",
  "src/panels/cleanup/useCleanupScan.ts",
  "src/panels/cleanup/DriveWipeDialog.tsx",
  "src/panels/cleanup/useCleanupLegacyDialogs.tsx",
  "src/panels/dev/index.tsx",
  "src/components/settings/VersionManagementCard.tsx",
  "src/panels/privacy/CanaryTokensSection.tsx",
  "src/panels/privacy/LogViewer.tsx",
  "src/panels/vault/RamDisksSection.tsx",
  "src/panels/privacy/AuthAnomalySection.tsx",
  "src/panels/privacy/PasteMonitorSection.tsx",
  "src/panels/privacy/RansomwareMonitorSection.tsx",
  "src/panels/privacy/RemoteAccessMonitorSection.tsx",
];

describe("application confirmation surface", () => {
  test("production frontend code never falls back to a native confirmation", () => {
    for (const path of productionSourceFiles(`${root}src`)) {
      expect(readFileSync(path, "utf8")).not.toContain("window.confirm(");
    }
  });

  test("all guarded destructive-action callers use the accessible app dialog", () => {
    for (const path of guardedActionCallers) {
      const source = readFileSync(`${root}${path}`, "utf8");
      expect(source).not.toContain("window.confirm(");
      expect(source).toContain("useAppConfirm");
    }
  });

  test("the main application owns one confirmation provider", () => {
    const entry = readFileSync(`${root}src/main.tsx`, "utf8");
    const main = readFileSync(`${root}src/entries/mainWindow.tsx`, "utf8");
    expect(entry).toContain('import("./entries/mainWindow")');
    expect(main).toContain("<AppConfirmProvider>");
    expect(main).toContain("</AppConfirmProvider>");
  });

  test("canary removal uses the Pro command contract and states artifact retention", () => {
    const source = readFileSync(`${root}src/panels/privacy/CanaryTokensSection.tsx`, "utf8");
    expect(source).toContain("invoke('delete_canary', { args: { id } })");
    expect(source).toContain("The generated artifact remains");
    expect(source).not.toContain("args: { tokenId: id }");
  });

  test("new requests queue while an older confirmation is waiting to be promoted", async () => {
    const { shouldQueueConfirmation } = await import("./AppConfirmDialog");
    expect(shouldQueueConfirmation(false, true, 1)).toBe(true);
    expect(shouldQueueConfirmation(false, false, 1)).toBe(true);
    expect(shouldQueueConfirmation(false, false, 0)).toBe(false);
  });
});
