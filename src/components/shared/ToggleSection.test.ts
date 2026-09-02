import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { orderTogglesForDisplay } from "./ToggleSection";
import type { ToggleDef } from "../../types/toggles";

function toggle(id: string): ToggleDef {
  return {
    id,
    label: id,
    description: id,
    section: "privacy",
    settingsPath: `ideal.${id}`,
    currentPath: `current.${id}`,
    enableCmd: "Enable-Test",
    disableCmd: "Disable-Test",
    tier: "free",
    needsAdmin: false,
    irreversible: false,
    reducesSecurity: false,
    defenderFlagged: false,
    domain: "privacy",
  };
}

describe("ToggleSection", () => {
  test("keeps toggles in source order when checked state changes", () => {
    const toggles = [toggle("off-a"), toggle("on-a"), toggle("off-b"), toggle("on-b")];

    expect(
      orderTogglesForDisplay(toggles, (item) => item.id.startsWith("on")).map((item) => item.id),
    ).toEqual(["off-a", "on-a", "off-b", "on-b"]);
  });

  test("checks the elevation guard before dispatching a privileged backend command", () => {
    const source = readFileSync("src/components/shared/ToggleSection.tsx", "utf8");
    const guard = source.indexOf("if (isPrivilegedWriteBlocked(toggle.needsAdmin, systemInfo?.isAdmin))");
    const dispatch = source.indexOf("executeBackendCommand(\"Set-AppCapabilityAccess\"");

    expect(guard).toBeGreaterThanOrEqual(0);
    expect(dispatch).toBeGreaterThan(guard);
    expect(source).toContain("needsElevation ||");
    expect(source).toContain("MACHINE_SCOPE_ELEVATION_MESSAGE");
  });
});
