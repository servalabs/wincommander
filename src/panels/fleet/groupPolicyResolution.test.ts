import { describe, expect, test } from "bun:test";
import { resolveGroupPolicy } from "./groupPolicyResolution";

const assignments = [
  { groupId: "marketing", value: "read" },
  { groupId: "developers", value: "write" },
];

describe("future feature policy conflict resolution", () => {
  test("Vault-style ranked access uses the highest grant", () => {
    expect(resolveGroupPolicy(["marketing", "developers"], assignments, {
      mode: "ranked",
      order: ["none", "read", "write"],
    })).toMatchObject({ status: "resolved", value: "write", hadDifferentValues: true });
  });

  test("security switches choose their declared safer value", () => {
    expect(resolveGroupPolicy(["marketing", "developers"], [
      { groupId: "marketing", value: "off" },
      { groupId: "developers", value: "on" },
    ], { mode: "secure-value", secureValue: "on" })).toMatchObject({
      status: "resolved",
      value: "on",
      hadDifferentValues: true,
    });
  });

  test("exclusive features block unresolved mixed values", () => {
    expect(resolveGroupPolicy(["marketing", "developers"], assignments, { mode: "manual" }))
      .toMatchObject({ status: "conflict", hadDifferentValues: true });
  });

  test("priority features use the first matching group", () => {
    expect(resolveGroupPolicy(["marketing", "developers"], assignments, {
      mode: "priority",
      groupOrder: ["marketing", "developers"],
    })).toMatchObject({ status: "resolved", value: "read", sourceGroupIds: ["marketing"] });
  });

  test("unassigned users stay unset", () => {
    expect(resolveGroupPolicy([], assignments, { mode: "manual" })).toEqual({
      status: "unset",
      sourceGroupIds: [],
      hadDifferentValues: false,
    });
  });

  test("unknown ranked values fail closed even when all groups agree", () => {
    expect(resolveGroupPolicy(["marketing"], [{ groupId: "marketing", value: "owner" }], {
      mode: "ranked",
      order: ["none", "read", "write"],
    })).toMatchObject({ status: "invalid" });
  });

  test("duplicate assignments for one group are invalid", () => {
    expect(resolveGroupPolicy(["marketing"], [
      { groupId: "marketing", value: "read" },
      { groupId: "marketing", value: "write" },
    ], { mode: "manual" })).toMatchObject({ status: "invalid" });
  });
});
