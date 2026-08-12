import { describe, expect, test } from "bun:test";
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
});
