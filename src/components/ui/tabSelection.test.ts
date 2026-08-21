import { describe, expect, test } from "bun:test";
import { resolveAvailableTab } from "./tabSelection";

describe("resolveAvailableTab", () => {
  test("selects the first available tab before async state catches up", () => {
    expect(resolveAvailableTab(["applications", "browsers"], undefined)).toBe("applications");
  });

  test("preserves a current tab while it remains available", () => {
    expect(resolveAvailableTab(["winget", "scoop"], "scoop")).toBe("scoop");
  });

  test("moves away from a tab removed by refreshed results", () => {
    expect(resolveAvailableTab(["winget", "npm"], "scoop")).toBe("winget");
  });

  test("uses a preferred fallback when async results first appear", () => {
    expect(resolveAvailableTab(["winget", "scoop"], undefined, "scoop")).toBe("scoop");
  });

  test("returns undefined only when no tab can be rendered", () => {
    expect(resolveAvailableTab([], "winget")).toBeUndefined();
  });
});
