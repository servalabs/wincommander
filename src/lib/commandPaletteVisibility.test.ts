import { describe, expect, test } from "bun:test";
import {
  isCommandPaletteActionVisible,
  isCommandPaletteFeatureVisible,
  isCommandPalettePanelVisible,
  type CommandPaletteVisibility,
} from "./commandPaletteVisibility";

const visible: CommandPaletteVisibility = {
  alwaysHiddenPanels: [],
  lockedPanels: [],
  alwaysHiddenActions: [],
  borrowedHidden: [],
  borrowedActive: false,
};

describe("command palette visibility", () => {
  test("never lists always-hidden panel destinations, including Search Files", () => {
    const config = {
      ...visible,
      alwaysHiddenPanels: ["privacy", "search-files"],
    };
    expect(isCommandPalettePanelVisible("privacy", config)).toBe(false);
    expect(isCommandPalettePanelVisible("search-files", config)).toBe(false);
  });

  test("hides borrowed-locked panels only while borrowed mode is active", () => {
    const config = { ...visible, lockedPanels: ["network"] };
    expect(isCommandPalettePanelVisible("network", config)).toBe(true);
    expect(isCommandPalettePanelVisible("network", { ...config, borrowedActive: true })).toBe(false);
  });

  test("filters quick actions marked always hidden", () => {
    expect(isCommandPaletteActionVisible("ai-advisor", {
      ...visible,
      alwaysHiddenActions: ["ai-advisor"],
    })).toBe(false);
  });

  test("filters borrowed-only actions and features only in borrowed mode", () => {
    const config = { ...visible, borrowedHidden: ["action:delete", "notif-bell"] };
    expect(isCommandPaletteActionVisible("delete", config)).toBe(true);
    expect(isCommandPaletteFeatureVisible("notif-bell", config)).toBe(true);
    expect(isCommandPaletteActionVisible("delete", { ...config, borrowedActive: true })).toBe(false);
    expect(isCommandPaletteFeatureVisible("notif-bell", { ...config, borrowedActive: true })).toBe(false);
  });
});
