import { describe, expect, test } from "bun:test";
import {
  hasExpandedDuplicateGroups,
  isDuplicateGroupExpanded,
  setDuplicateGroupExpanded,
  toggleDuplicateGroups,
} from "./duplicateGroupPresentation";

describe("duplicate group presentation", () => {
  test("collapse all leaves group headers while hiding every member row", () => {
    const collapsed = toggleDuplicateGroups(null);
    expect(collapsed).toEqual(new Set());
    expect(isDuplicateGroupExpanded(collapsed, "one")).toBe(false);
    expect(hasExpandedDuplicateGroups(collapsed)).toBe(false);
  });

  test("expand all restores every loaded group after a collapsed view", () => {
    expect(toggleDuplicateGroups(new Set())).toBeNull();
  });

  test("an Enter search can reveal its first matching group without reopening unrelated groups", () => {
    const expanded = setDuplicateGroupExpanded(new Set(), ["first", "second"], "second", true);
    expect(isDuplicateGroupExpanded(expanded, "first")).toBe(false);
    expect(isDuplicateGroupExpanded(expanded, "second")).toBe(true);
  });
});
