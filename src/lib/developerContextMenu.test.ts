import { describe, expect, test } from "bun:test";
import { shouldAllowDeveloperContextMenu } from "./developerContextMenu";

describe("developer context-menu gate", () => {
  test("requires both a native debug build and an explicit opt-in", () => {
    expect(shouldAllowDeveloperContextMenu(true, true)).toBe(true);
    expect(shouldAllowDeveloperContextMenu(true, false)).toBe(false);
    expect(shouldAllowDeveloperContextMenu(true, undefined)).toBe(false);
  });

  test("fails closed in production even if a saved preference says enabled", () => {
    expect(shouldAllowDeveloperContextMenu(false, true)).toBe(false);
  });
});
