import { describe, expect, test } from "bun:test";
import {
  mergeAccessUsers, reconcileAccessDirectoryUsers, validateAccessDirectory,
} from "./accessControlPolicy";
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
    expect(merged[0]).toMatchObject({ id: "sid:s-1", displayName: "Alex Morgan", sid: "S-1" });
  });

  test("reconciles a renamed Windows account by SID without losing memberships", () => {
    const directory: FleetAccessDirectory = {
      schema: 1,
      users: [{
        id: "administrator",
        username: "Administrator",
        displayName: "Administrator",
        sid: "S-1-5-21-100-500",
        isCurrent: true,
      }],
      groups: [{
        id: "admins",
        name: "Administrators",
        localGroup: "WC_Administrators",
        userIds: ["administrator"],
      }],
    };

    const reconciled = reconcileAccessDirectoryUsers(directory, [{
      id: "sid:s-1-5-21-100-500",
      username: "Admin",
      displayName: "Admin",
      sid: "S-1-5-21-100-500",
      isCurrent: true,
      isAvailable: true,
    }]);

    expect(reconciled.users).toEqual([{
      id: "sid:s-1-5-21-100-500",
      username: "Admin",
      displayName: "Admin",
      sid: "S-1-5-21-100-500",
      isCurrent: true,
      isAvailable: true,
    }]);
    expect(reconciled.groups[0]?.userIds).toEqual(["sid:s-1-5-21-100-500"]);
    expect(validateAccessDirectory(reconciled)).toEqual([]);
  });

  test("does not merge different Windows accounts that reuse a name", () => {
    const reconciled = reconcileAccessDirectoryUsers({
      schema: 1,
      users: [{ id: "old", username: "Admin", sid: "S-1-5-21-100-500" }],
      groups: [],
    }, [{ id: "new", username: "Admin", sid: "S-1-5-21-100-1001" }]);

    expect(reconciled.users).toHaveLength(2);
    expect(new Set(reconciled.users.map(user => user.id))).toEqual(new Set([
      "sid:s-1-5-21-100-500",
      "sid:s-1-5-21-100-1001",
    ]));
  });

  test("does not bridge two SID accounts through a legacy name-only row", () => {
    const reconciled = reconcileAccessDirectoryUsers({
      schema: 1,
      users: [
        { id: "old-admin", username: "Admin", sid: "S-1-5-21-100-500" },
        { id: "legacy-admin", username: "Admin" },
      ],
      groups: [{ id: "ops", name: "Ops", localGroup: "WC_Ops", userIds: ["legacy-admin"] }],
    }, [{ id: "new-admin", username: "Admin", sid: "S-1-5-21-100-1001" }]);

    expect(reconciled.users).toHaveLength(3);
    expect(reconciled.groups[0]?.userIds).toEqual(["legacy-admin"]);
    expect(reconciled.users.find(user => user.id === "legacy-admin")?.isAvailable).toBe(false);
  });

  test("keeps accounts absent from successful discovery as unavailable and unassignable", () => {
    const reconciled = reconcileAccessDirectoryUsers({
      schema: 1,
      users: [{ id: "sid:s-1-old", username: "Old user", sid: "S-1-old" }],
      groups: [{ id: "ops", name: "Ops", localGroup: "WC_Ops", userIds: ["sid:s-1-old"] }],
    }, [{ id: "sid:s-1-current", username: "Current user", sid: "S-1-current", isCurrent: true }]);

    expect(reconciled.users.find(user => user.id === "sid:s-1-old")?.isAvailable).toBe(false);
    expect(validateAccessDirectory(reconciled)).toContain("A group contains a Windows account that is disabled or deleted.");
  });

});
