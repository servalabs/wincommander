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

  test("unions each write with the latest persisted list", () => {
    const first = addIgnoredFindingId([], "first");
    const second = addIgnoredFindingId(first, "second");

    expect(second).toEqual(["first", "second"]);
  });

  test("restoring one item leaves other ignored findings intact", () => {
    expect(removeIgnoredFindingId(["first", "second"], "first")).toEqual(["second"]);
  });
});
