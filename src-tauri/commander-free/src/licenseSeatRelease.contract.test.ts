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

test("an unconfirmed seat release leaves the app and service working and explains itself", async () => {
  const [hooks, license, lib] = await Promise.all([
    read("src-tauri/commander-free/nsis/hooks.nsh"),
    read("src-tauri/commander-free/src/license.rs"),
    read("src-tauri/commander-free/src/lib.rs"),
  ]);
  const uninstallHook = hooks.slice(
    hooks.indexOf("!macro NSIS_HOOK_PREUNINSTALL"),
    hooks.indexOf("!macroend", hooks.indexOf("!macro NSIS_HOOK_PREUNINSTALL")),
  );
  const release = uninstallHook.indexOf("!insertmacro WC_RELEASE_LICENSE_SEAT_OR_ABORT");
  expect(release).toBeGreaterThan(-1);
  expect(uninstallHook.match(/!insertmacro WC_RELEASE_LICENSE_SEAT_OR_ABORT/g)).toHaveLength(1);
  // Nothing may be closed, stopped or deleted before the release is confirmed.
  for (const later of [
    '!insertmacro WC_CLOSE_OWNED_DESKTOP_APP_OR_ABORT "uninstall"',
    '!insertmacro WC_STOP_OWNED_SERVICE_OR_ABORT "uninstall" "un"',
    '!insertmacro WC_DELETE_OWNED_SERVICE_OR_ABORT "uninstall"',
    "!insertmacro WC_REMOVE_AUTOSTART_ROUTES_OR_ABORT",
    'Delete "${WC_CALCULATOR_EXE}"',
  ]) {
    expect(uninstallHook.indexOf(later)).toBeGreaterThan(release);
  }
  // Only a real uninstall releases: the call sits inside the `/UPDATE` error branch.
  const guard = uninstallHook.slice(0, release);
  expect(guard).toMatch(/\$\{GetOptions\} \$CMDLINE "\/UPDATE" \$R7\r?\n\s+\$\{If\} \$\{Errors\}\r?\n\s*$/);

  const macro = hooks.slice(
    hooks.indexOf("!macro WC_RELEASE_LICENSE_SEAT_OR_ABORT"),
    hooks.indexOf("!macroend", hooks.indexOf("!macro WC_RELEASE_LICENSE_SEAT_OR_ABORT")),
  );
  expect(macro).toContain('WC_WRITE_LIFECYCLE_DIAGNOSTIC "uninstall-license-release" "$0" "$R6"');
  expect(macro).toContain("${WC_LICENCE_RELEASE_RESULT}");
  expect(macro).toContain("Reason: $R7");
  expect(macro).toContain("MB_YESNO|MB_ICONEXCLAMATION|MB_DEFBUTTON2");
  expect(macro).toContain("Could not confirm release of this device's licence seat ($R7)");
  expect(macro).toContain("release the device later from your WinCommander account");
  // Silent uninstall must never reach the continue prompt.
  expect(macro.indexOf("IfSilent wc_license_release_abort")).toBeGreaterThan(-1);
  expect(macro.indexOf("IfSilent wc_license_release_abort")).toBeLessThan(macro.indexOf("MessageBox"));
  // Only offline (2), 5xx/429/408 (3 + server_unavailable) are offered a choice; 4 continues; the rest abort.
  expect(macro).toContain("reason=server_unavailable");
  expect(macro).toMatch(/\$\{If\} \$0 == 4\r?\n[^\n]*\r?\n\s+Goto wc_license_release_done/);
  expect(macro).toMatch(/\$\{If\} \$0 == 2\r?\n\s+Goto wc_license_release_uncertain/);
  expect(macro.slice(macro.indexOf("wc_license_release_abort:"))).toContain("Abort ");
  expect(macro).not.toContain('"attempted"');

  expect(hooks).toContain('!define WC_LICENCE_RELEASE_RESULT "$INSTDIR\\licence-release-result.txt"');
  expect(license).toContain('const RELEASE_RESULT_FILE: &str = "licence-release-result.txt";');
  expect(lib).toContain("license::write_release_result_file(&line)");
});
