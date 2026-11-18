import { describe, expect, test } from "bun:test";
import { sidebarHiddenActions } from "./visibilityDefaults";

describe("visibility defaults", () => {
  test("starts AI Advisor hidden until the user enables it", () => {
    expect(sidebarHiddenActions([], undefined)).toContain("ai-advisor");
    expect(sidebarHiddenActions([], true)).not.toContain("ai-advisor");
    expect(sidebarHiddenActions(["ai-advisor"], true)).toContain("ai-advisor");
  });
});
