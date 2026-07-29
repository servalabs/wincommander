import { expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

const read = (path: string) => Bun.file(path).text();

test("Debloat prefers original locally installed icons before bundled fallbacks", async () => {
  const [panel, inventory, icon, utils, uninstaller, bcu] = await Promise.all([
    read("src/panels/apps/DebloatPanel.tsx"),
    read("src/panels/apps/useDebloatInventory.ts"),
    read("src/panels/apps/components/AppIcon.tsx"),
    read("src-tauri/commander-free/scripts/core/utils.ps1"),
    read("src-tauri/commander-free/scripts/modules/apps/uninstaller.ps1"),
    read("src-tauri/commander-free/scripts/modules/apps/bcu-uninstaller.ps1"),
  ]);

  expect(panel).toContain("preferNative");
  expect(panel).toContain("iconData={item.iconData}");
  expect(inventory).toContain("getDebloatWindowsIconData");
  expect(inventory).toContain("iconData: a.iconData");
  expect(icon.indexOf("if (preferNative && iconData)") < icon.indexOf("if (attempt < candidates.length)")).toBe(true);
  expect(utils).toContain("function Get-WcAppxIconData");
  expect(utils).toContain("function Get-WcExecutableIconData");
  expect(uninstaller).toContain("function Get-DebloatWindowsIconData");
  expect(uninstaller).toContain("iconData        = Get-WcAppxIconData");
  expect(bcu).toContain("Get-WcExecutableIconData");
});
