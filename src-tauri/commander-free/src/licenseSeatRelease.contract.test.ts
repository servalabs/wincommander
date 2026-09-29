import { expect, test } from "bun:test";

const read = (path: string) => Bun.file(path).text();

test("an explicit uninstall releases its seat while an update preserves it", async () => {
  const [hooks, main, quickPanel, license] = await Promise.all([
    read("src-tauri/commander-free/nsis/hooks.nsh"),
    read("src-tauri/commander-free/src/main.rs"),
    read("src/components/LicenseQuickPanel.tsx"),
    read("src-tauri/commander-free/src/license.rs"),
  ]);

  expect(hooks).toContain("!macro WC_RELEASE_LICENSE_SEAT_OR_ABORT");
  expect(hooks).toContain('${GetOptions} $CMDLINE "/UPDATE" $R7');
  expect(hooks).toContain("!insertmacro WC_RELEASE_LICENSE_SEAT_OR_ABORT");
  expect(hooks).toContain("--release-license-seat");
  expect(main).toContain("run_license_seat_release_if_requested");
  expect(license).toContain("pub async fn release_license_seat_if_present");
  expect(license).toContain("the licence was kept so you can retry");
  const clearCacheCommand = license.slice(
    license.indexOf("pub async fn clear_license_cache"),
    license.indexOf("/// Start the one-time", license.indexOf("pub async fn clear_license_cache")),
  );
  expect(clearCacheCommand).toContain('if claims.plan == "trial"');
  expect(clearCacheCommand).toContain("deactivate_license_internal().await");
  expect(quickPanel).not.toContain("clearAppLicenseCache");
});
