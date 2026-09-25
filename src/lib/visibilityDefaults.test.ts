import { describe, expect, test } from "bun:test";
import {
  DEFAULT_ALWAYS_HIDDEN_SIDEBAR_ACTIONS,
  DEFAULT_BORROWED_EXTRAS,
} from "./visibilityDefaults";

describe("visibility defaults", () => {
  test("hides license controls and the tour in Borrowed Mode by default", () => {
    expect(DEFAULT_BORROWED_EXTRAS).toContain("license-panel");
    expect(DEFAULT_BORROWED_EXTRAS).toContain("tour");
  });

  test("conceals the other requested surfaces in Borrowed Mode", () => {
    for (const surface of [
      "risk-matrix",
      "more-products",
      "notif-bell",
      "popup-alerts",
      "desktop-alerts",
      "sidebar-preferences",
      "license-panel",
      "tour",
    ]) {
      expect(DEFAULT_BORROWED_EXTRAS).toContain(surface);
    }
  });

  test("starts AI Advisor hidden until the user enables it", () => {
    expect(DEFAULT_ALWAYS_HIDDEN_SIDEBAR_ACTIONS).toEqual(["ai-advisor"]);
  });
});
