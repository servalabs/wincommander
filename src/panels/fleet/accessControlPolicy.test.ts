import { describe, expect, test } from "bun:test";
import { mergeAccessUsers, validateAccessDirectory } from "./accessControlPolicy";
import type { FleetAccessDirectory } from "./accessControlTypes";

describe("Fleet universal access groups", () => {
  test("allows one Windows user in several groups", () => {
    const directory: FleetAccessDirectory = {
      schema: 1,
      users: [{ id: "alex", username: "Alex" }],
      groups: [
        { id: "marketing", name: "Marketing", localGroup: "WC_Marketing", userIds: ["alex"] },
        { id: "developers", name: "Developers", localGroup: "WC_Developers", userIds: ["alex"] },
      ],
    };
    expect(validateAccessDirectory(directory)).toEqual([]);
  });

  test("rejects duplicate group names and missing Windows users", () => {
    const directory: FleetAccessDirectory = {
      schema: 1,
      users: [],
      groups: [
        { id: "one", name: "Team", localGroup: "WC_One", userIds: ["missing"] },
        { id: "two", name: "team", localGroup: "WC_Two", userIds: [] },
      ],
    };
    expect(validateAccessDirectory(directory)).toContain("Group names must be unique.");
    expect(validateAccessDirectory(directory)).toContain("A group contains a Windows user that is no longer available.");
  });

  test("merges discovered profiles case-insensitively", () => {
    const merged = mergeAccessUsers(
      [{ id: "alex", username: "Alex" }],
      [{ id: "ALEX", username: "ALEX", displayName: "Alex Morgan", sid: "S-1" }],
    );
    expect(merged).toHaveLength(1);
    expect(merged[0]).toMatchObject({ id: "alex", displayName: "Alex Morgan", sid: "S-1" });
  });
});
