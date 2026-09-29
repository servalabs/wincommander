import { expect, test } from "bun:test";
import { accessGroupSaveFailure } from "./accessGroupSaveFailure";

test("group outsider denial does not suggest that administrator elevation grants membership", () => {
  expect(accessGroupSaveFailure("vault_not_authorized: PRIVATE_RAW_DETAIL")).toContain("must already belong");
  expect(accessGroupSaveFailure("vault_not_authorized: PRIVATE_RAW_DETAIL")).not.toContain("PRIVATE_RAW_DETAIL");
});

test("mounted groups require an authorized dismount and unknown failures stay bounded", () => {
  expect(accessGroupSaveFailure(new Error("vault_policy_mounted"))).toContain("authorized group member");
  expect(accessGroupSaveFailure("C:\\private\\secret")).not.toContain("secret");
});

test("group reuse and referenced deletion have distinct recovery actions", () => {
  expect(accessGroupSaveFailure("vault_group_in_use")).toContain("still assigned to a Vault");
  expect(accessGroupSaveFailure("vault_group_name_conflict")).toContain("Choose a different name");
  expect(accessGroupSaveFailure("vault_legacy_group_wire_retired")).toContain("Reload Fleet");
});
