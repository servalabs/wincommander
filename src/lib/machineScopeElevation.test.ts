import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { ALL_TOGGLES } from "../registry";
import { isPrivilegedWriteBlocked } from "./machineScopeElevation";

const scopeSource = readFileSync("src-tauri/commander-free/src/settings_scope.rs", "utf8");
const machinePaths = new Set(
  [...scopeSource.matchAll(/path:\s*"([^"]+)",\s*scope:\s*SettingsScope::Machine/g)].map((match) => match[1]),
);

describe("machine-scope frontend fallback", () => {
  test("does not dispatch a machine-scoped registry control for a standard Windows user", () => {
    const machineControls = ALL_TOGGLES.filter((toggle) =>
      machinePaths.has(toggle.settingsPath.replace(/^ideal\./, "")),
    );

    expect(machineControls.length).toBeGreaterThan(0);
    for (const toggle of machineControls) {
      expect(toggle.needsAdmin).toBe(true);
      expect(isPrivilegedWriteBlocked(toggle.needsAdmin, false)).toBe(true);
    }
  });

  test("fails closed while the Windows account capability is still unknown", () => {
    expect(isPrivilegedWriteBlocked(true, undefined)).toBe(true);
    expect(isPrivilegedWriteBlocked(false, undefined)).toBe(false);
  });
});
