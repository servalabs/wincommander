import { describe, expect, test } from "bun:test";
import { getToggleById } from "./index";

describe("Explorer setup defaults", () => {
  test("selects the requested Explorer settings by default", () => {
    const mappings = [
      ["syncNotif", "ideal.tweaks.ui.syncProviderNotificationsHidden", "Hide-SyncProviderNotifications", "Show-SyncProviderNotifications"],
      ["enthusiastMode", "ideal.tweaks.performance.enthusiastModeEnabled", "Enable-EnthusiastMode", "Disable-EnthusiastMode"],
      ["icoThisPc", "ideal.tweaks.ui.desktopIconThisPc", "Show-DesktopIconThisPc", "Hide-DesktopIconThisPc"],
      ["icoRecycle", "ideal.tweaks.ui.desktopIconRecycleBin", "Show-DesktopIconRecycleBin", "Hide-DesktopIconRecycleBin"],
      ["clockSeconds", "ideal.tweaks.ui.clockSecondsVisible", "Show-ClockSeconds", "Hide-ClockSeconds"],
      ["explorerPC", "ideal.tweaks.ui.explorerOpensThisPc", "Set-ExplorerOpensThisPC", "Set-ExplorerOpensQuickAccess"],
    ] as const;

    for (const [id, settingsPath, enableCmd, disableCmd] of mappings) {
      const toggle = getToggleById(id);
      expect(toggle !== undefined).toBe(true);
      expect(toggle?.defaultOn).toBe(true);
      expect(toggle?.settingsPath).toBe(settingsPath);
      expect(toggle?.enableCmd).toBe(enableCmd);
      expect(toggle?.disableCmd).toBe(disableCmd);
      expect(toggle?.radar).toBe(true);
    }
  });
});
