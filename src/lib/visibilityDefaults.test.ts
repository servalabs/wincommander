import { describe, expect, test } from "bun:test";
import {
  DEFAULT_ALWAYS_HIDDEN_SIDEBAR_ACTIONS,
  DEFAULT_BORROWED_EXTRAS,
} from "./visibilityDefaults";

describe("visibility defaults", () => {
  test("conceals the requested surfaces in Borrowed Mode", () => {
    expect(DEFAULT_BORROWED_EXTRAS).toEqual(expect.arrayContaining([
      "risk-matrix",
      "more-products",
      "notif-bell",
      "popup-alerts",
      "desktop-alerts",
      "sidebar-preferences",
    ]));
  });

  test("starts AI Advisor hidden until the user enables it", () => {
    expect(DEFAULT_ALWAYS_HIDDEN_SIDEBAR_ACTIONS).toEqual(["ai-advisor"]);
  });
});
