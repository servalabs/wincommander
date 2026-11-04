import { expect, test } from "bun:test";
import { duplicateProfileKey, duplicateProfileLabel, isDuplicateGroupsFiltered } from "./duplicateProfile";

const base = { id: "id", name: "file", path: "D:\\Redirected\\Alice\\file", size: 1, modified: null };

test("uses registered redirected profile rather than guessing from the path", () => {
  const file = { ...base, profileScope: "other" as const, profileRoot: "D:\\Redirected\\Alice" };
  expect(duplicateProfileKey(file)).toBe("other:D:\\Redirected\\Alice");
  expect(duplicateProfileLabel(file)).toContain("Other user");
});

test("bulk selection is disabled whenever groups are filtered", () => {
  expect(isDuplicateGroupsFiltered("", "all", "all")).toBe(false);
  expect(isDuplicateGroupsFiltered("report", "all", "all")).toBe(true);
  expect(isDuplicateGroupsFiltered("", "images", "all")).toBe(true);
  expect(isDuplicateGroupsFiltered("", "all", "other:D:\\Redirected\\Alice")).toBe(true);
});

test("treats older or incomplete backend classifications as unavailable", () => {
  expect(duplicateProfileKey(base)).toBe("unavailable");
  expect(duplicateProfileLabel({ ...base, profileScope: "current" })).toBe("Profile unavailable");
  expect(duplicateProfileKey({ ...base, profileScope: "outside" })).toBe("outside");
});
