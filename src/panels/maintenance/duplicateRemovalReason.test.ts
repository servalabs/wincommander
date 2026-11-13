import { expect, test } from "bun:test";
import { duplicateRemovalReason } from "./duplicateRemovalReason";

test("explains why inaccessible profiles block duplicate removal", () => {
  expect(duplicateRemovalReason("profiles_unavailable")).toContain("profiles could not be verified");
});

test("describes the limited Git metadata protection", () => {
  expect(duplicateRemovalReason("git_metadata")).toBe("Files inside .git directories are protected.");
});

test("never displays unrecognized backend text", () => {
  expect(duplicateRemovalReason("C:\\private\\unexpected")).not.toContain("private");
  expect(duplicateRemovalReason(undefined)).toContain("cannot be verified");
});
