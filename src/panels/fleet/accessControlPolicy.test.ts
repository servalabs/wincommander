import { describe, expect, test } from "bun:test";
import {
  buildAccessGroupReconcilePlan, describeReconcileFailure, mergeAccessUsers,
  reconcileAccessDirectoryUsers, summarizeReconcileResults, validateAccessDirectory,
} from "./accessControlPolicy";
import type { AccessGroupReconcileResult, FleetAccessDirectory } from "./accessControlTypes";

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

describe("buildAccessGroupReconcilePlan — SID collection for the Windows group reconcile request", () => {
  const directory: FleetAccessDirectory = {
    schema: 1,
    users: [
      { id: "sid:s-1-1", username: "Alex", sid: "S-1-1" },
      { id: "sid:s-1-2", username: "Sam", sid: "S-1-2" },
      { id: "legacy:pat", username: "Pat" }, // no SID — never discovered, or discovery failed
    ],
    groups: [
      { id: "sales", name: "Sales", localGroup: "WC_Sales", userIds: ["sid:s-1-1", "sid:s-1-2"] },
      { id: "ops", name: "Ops", localGroup: "WC_Ops", userIds: ["legacy:pat"] },
      { id: "empty", name: "Empty", localGroup: "WC_Empty", userIds: [] },
    ],
  };

  test("sends each group's Windows local group name with its members' SIDs", () => {
    const plan = buildAccessGroupReconcilePlan(directory);
    expect(plan.requests).toEqual([
      { local_group: "WC_Sales", member_sids: ["S-1-1", "S-1-2"] },
      { local_group: "WC_Ops", member_sids: [] },
      { local_group: "WC_Empty", member_sids: [] },
    ]);
  });

  test("skips and flags a member with no SID rather than sending a name", () => {
    const plan = buildAccessGroupReconcilePlan(directory);
    expect(plan.skippedMembers).toEqual([{ groupName: "Ops", count: 1 }]);
    expect(plan.requests.find(request => request.local_group === "WC_Ops")?.member_sids).not.toContain("Pat");
  });
});

describe("summarizeReconcileResults — honest per-group reporting", () => {
  test("reports success only when every group succeeded", () => {
    const results: AccessGroupReconcileResult[] = [
      { local_group: "WC_Sales", state: "created", error: null },
      { local_group: "WC_Ops", state: "unchanged", error: null },
    ];
    const outcome = summarizeReconcileResults(results);
    expect(outcome.intent).toBe("success");
  });

  test("a single failed group is never hidden behind a blanket success toast", () => {
    const results: AccessGroupReconcileResult[] = [
      { local_group: "WC_Sales", state: "created", error: null },
      { local_group: "WC_Ops", state: "failed", error: "access denied creating local group" },
    ];
    const outcome = summarizeReconcileResults(results);
    expect(outcome.intent).toBe("danger");
    expect(outcome.message).toContain("WC_Ops");
    expect(outcome.message).toContain("access denied creating local group");
    expect(outcome.message).toContain("1 of 2");
  });
});

describe("describeReconcileFailure — unprivileged-caller and unreachable-service paths", () => {
  test("a forbidden rejection from an unprivileged caller reads as needing an administrator", () => {
    // Matches svc_client::call's `format!("service rejected request: {}", error.kind)`
    // for the service's `kind: "forbidden"` ErrorReply on a non-admin caller.
    const message = describeReconcileFailure(new Error("service rejected request: forbidden"));
    expect(message).toContain("needs an administrator");
  });

  test("a transport failure reads as the service being unreachable, not a silent failure", () => {
    const message = describeReconcileFailure(new Error("service connect failed: The system cannot find the file specified."));
    expect(message).toContain("were not created");
    expect(message).not.toContain("needs an administrator");
  });

  test("handles a plain string rejection the same as an Error", () => {
    expect(describeReconcileFailure("service rejected request: forbidden")).toContain("needs an administrator");
  });
});
