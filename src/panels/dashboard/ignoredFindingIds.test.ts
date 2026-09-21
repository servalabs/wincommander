import { describe, expect, test } from "bun:test";
import {
  addIgnoredFindingId,
  effectiveIgnoredFindingIds,
  removeIgnoredFindingId,
} from "./ignoredFindingIds";

describe("dashboard ignored findings", () => {
  test("hides every rapid click before settings writes finish", () => {
    expect(effectiveIgnoredFindingIds(["already-saved"], ["first", "second", "first"]))
      .toEqual(["already-saved", "first", "second"]);
  });

  test("restores immediately even while the persisted ignore ID is still present", () => {
    expect(effectiveIgnoredFindingIds(["first", "second"], [], ["first"]))
      .toEqual(["second"]);
  });

  test("the latest click wins when restore and ignore overlap", () => {
    expect(effectiveIgnoredFindingIds(["first"], ["first"], []))
      .toEqual(["first"]);
  });

  test("unions each write with the latest persisted list", () => {
    const first = addIgnoredFindingId([], "first");
    const second = addIgnoredFindingId(first, "second");

    expect(second).toEqual(["first", "second"]);
  });

  test("restoring one item leaves other ignored findings intact", () => {
    expect(removeIgnoredFindingId(["first", "second"], "first")).toEqual(["second"]);
  });
});
