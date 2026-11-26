import { describe, expect, test } from "bun:test";
import { buildPasswordChangeRequest, buildPasswordProbeRequest } from "./vaultPasswordChange";
import {
  DECOY_PASSWORD_CHANGED,
  PASSWORD_CHANGED,
  REAL_PASSWORD_CHANGED,
  passwordChangeProblem,
  passwordChangeReducer,
  startPasswordChange,
  type PasswordChangeEvent,
  type PasswordChangeFlow,
} from "./vaultPasswordChangeFlow";

const PATH = "D:\\Vaults\\notes.hc";

const run = (flow: PasswordChangeFlow, ...events: PasswordChangeEvent[]) => events.reduce(passwordChangeReducer, flow);
const type = (part: "first" | "decoy", values: Record<string, string>): PasswordChangeEvent[] =>
  Object.entries(values).map(([field, value]) => ({ type: "edit", part, field: field as "oldPassword", value }));

const probedAs = (header: "outer" | "hidden") => run(startPasswordChange(PATH),
  ...type("first", { oldPassword: "current" }), { type: "send" }, { type: "probed", header },
  ...type("first", { newPassword: "fresh", confirmPassword: "fresh" }));

const realChanged = () => run(probedAs("hidden"), { type: "send" }, { type: "changed" });

describe("password change steps", () => {
  test("outer: one password, new password, then a plain success with no decoy wording", () => {
    const ready = probedAs("outer");
    expect(ready.step).toBe("new");
    expect(passwordChangeProblem(ready)).toBeNull();
    expect(buildPasswordChangeRequest(ready.volumePath, ready.header!, ready.first).Target).toBe("outer");
    const done = run(ready, { type: "send" }, { type: "changed" });
    expect(done.step).toBe("done");
    expect(done.success).toBe("Password changed.");
    expect(done.success).toBe(PASSWORD_CHANGED);
    for (const word of ["decoy", "real", "other password"]) expect(done.success.toLowerCase()).not.toContain(word);
    expect(done.first.oldPassword).toBe("");
  });

  test("hidden: the real password changes, then the decoy question; No ends with the real change only", () => {
    const ready = probedAs("hidden");
    expect(buildPasswordChangeRequest(ready.volumePath, ready.header!, ready.first).Target).toBe("hidden");
    const asked = realChanged();
    expect(asked.step).toBe("decoy-ask");
    expect(asked.success).toBe(REAL_PASSWORD_CHANGED);
    expect(asked.success).toContain("real password");
    expect(asked.success).toContain("decoy password still works");
    const declined = run(asked, { type: "decoy-no" });
    expect(declined.step).toBe("done");
    expect(declined.success).toBe(REAL_PASSWORD_CHANGED);
  });

  test("hidden then Yes: the decoy password changes as the outer header", () => {
    const decoy = run(realChanged(), { type: "decoy-yes" },
      ...type("decoy", { oldPassword: "old decoy", newPassword: "new decoy", confirmPassword: "new decoy" }));
    expect(decoy.step).toBe("decoy");
    expect(passwordChangeProblem(decoy)).toBeNull();
    expect(buildPasswordChangeRequest(decoy.volumePath, "outer", decoy.decoy)).toMatchObject({
      Target: "outer", OldPassword: "old decoy", NewPassword: "new decoy",
    });
    const done = run(decoy, { type: "send" }, { type: "changed" });
    expect(done.step).toBe("done");
    expect(done.success).toBe(DECOY_PASSWORD_CHANGED);
    expect(done.success).toContain("real password still works");
    expect(done.decoy.oldPassword).toBe("");
  });

  test("a wrong decoy password keeps the first change and asks again", () => {
    const decoy = run(realChanged(), { type: "decoy-yes" },
      ...type("decoy", { oldPassword: "bad", newPassword: "new decoy", confirmPassword: "new decoy" }));
    const refused = run(decoy, { type: "send" }, { type: "failed", error: new Error("wrong_password") });
    expect(refused.step).toBe("decoy");
    expect(refused.failure).toBe("Wrong password. The first password change was kept.");
    expect(refused.success).toBe(REAL_PASSWORD_CHANGED);
    expect(refused.decoy.oldPassword).toBe("");
    expect(refused.busy).toBe(false);
  });

  test("a wrong password shows only that and no new-password boxes", () => {
    const typed = run(startPasswordChange(PATH), ...type("first", { oldPassword: "nope" }));
    expect(buildPasswordProbeRequest(typed.volumePath, typed.first).Password).toBe("nope");
    const refused = run(typed, { type: "send" }, { type: "failed", error: "service rejected request: wrong_password" });
    expect(refused.step).toBe("current");
    expect(refused.header).toBeNull();
    expect(refused.failure).toBe("Wrong password.");
    expect(refused.first.oldPassword).toBe("");
  });

  test("a policy-managed vault stops at the Vault Permissions message", () => {
    const typed = run(startPasswordChange(PATH), ...type("first", { oldPassword: "current" }));
    const stopped = run(typed, { type: "send" }, { type: "failed", error: "vault_policy_managed" });
    expect(stopped.step).toBe("policy");
    expect(stopped.failure).toBe("This Vault is managed in Vault Permissions. Change its password there.");
    expect(stopped.first.oldPassword).toBe("");
    expect(passwordChangeProblem(stopped)).toBeNull();
  });

  test("a refused new password keeps the step and drops only the new passwords", () => {
    const refused = run(probedAs("outer"), { type: "send" }, { type: "failed", error: "weak_new_password" });
    expect(refused.step).toBe("new");
    expect(refused.failure).toContain("too weak");
    expect(refused.first.oldPassword).toBe("current");
    expect(refused.first.newPassword).toBe("");
  });

  test("the vault file is fixed once the probe has opened it", () => {
    const moved = run(probedAs("outer"), { type: "path", volumePath: "E:\\other.hc" });
    expect(moved.volumePath).toBe(PATH);
  });

  test("closing clears every password", () => {
    const reset = run(realChanged(), { type: "decoy-yes" }, ...type("decoy", { oldPassword: "x" }), { type: "reset" });
    expect(reset).toEqual(startPasswordChange());
  });
});
