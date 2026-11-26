import { describe, expect, test } from "bun:test";
import {
  EMPTY_HEADER_CREDENTIALS,
  buildPasswordChangeRequest,
  buildPasswordProbeRequest,
  currentPasswordProblem,
  isPolicyManagedError,
  newPasswordProblem,
  readProbeHeader,
  vaultPasswordChangeErrorMessage,
  type HeaderCredentials,
} from "./vaultPasswordChange";

const PATH = "D:\\Vaults\\notes.hc";

const header = (changes: Partial<HeaderCredentials> = {}): HeaderCredentials => ({
  ...EMPTY_HEADER_CREDENTIALS,
  oldPassword: "old secret",
  newPassword: "new secret",
  confirmPassword: "new secret",
  ...changes,
});

describe("current password form", () => {
  test("needs a vault file and a current password or keyfile", () => {
    expect(currentPasswordProblem(PATH, header())).toBeNull();
    expect(currentPasswordProblem("  ", header())).toBe("Choose the vault file.");
    expect(currentPasswordProblem(PATH, header({ oldPassword: "" }))).toBe("Enter the current password.");
    expect(currentPasswordProblem(PATH, header({ oldPassword: "", oldKeyfile: "D:\\keys\\old.key" }))).toBeNull();
  });

  test("rejects a PIM outside the engine's range", () => {
    for (const pim of ["0", "2147469", "12a", "-5", "1.5"]) {
      expect(currentPasswordProblem(PATH, header({ oldPim: pim }))).toContain("PIM");
    }
    expect(currentPasswordProblem(PATH, header({ oldPim: "2147468" }))).toBeNull();
  });
});

describe("new password form", () => {
  test("a complete form has no problem", () => {
    expect(newPasswordProblem(header())).toBeNull();
  });

  test("the new password must be typed the same way twice", () => {
    expect(newPasswordProblem(header({ confirmPassword: "new secreT" }))).toBe("The new passwords do not match.");
    expect(newPasswordProblem(header({ confirmPassword: "x" }), "decoy ")).toBe("The new decoy passwords do not match.");
  });

  test("asks for each missing piece by name", () => {
    expect(newPasswordProblem(header({ newPassword: "", confirmPassword: "" }))).toBe("Enter a new password.");
    expect(newPasswordProblem(header({ oldPassword: "" }), "decoy ")).toBe("Enter the current decoy password.");
    expect(newPasswordProblem(header({ newPassword: "", confirmPassword: "", newKeyfile: "D:\\keys\\new.key" }))).toBeNull();
  });

  test("the new credential must differ from the current one", () => {
    const same = header({ newPassword: "old secret", confirmPassword: "old secret" });
    expect(newPasswordProblem(same)).toBe("The new password must be different from the current one.");
    expect(newPasswordProblem({ ...same, newPim: "600" })).toBeNull();
    expect(newPasswordProblem({ ...same, newKeyfile: "D:\\keys\\new.key" })).toBeNull();
  });

  test("rejects a bad new PIM", () => {
    expect(newPasswordProblem(header({ newPim: "0" }))).toContain("PIM");
  });
});

describe("vault password requests", () => {
  test("the probe sends one password with its PIM and keyfile", () => {
    expect(buildPasswordProbeRequest(` "${PATH}" `, header({ oldPim: "500", oldKeyfile: " D:\\keys\\old.key " }))).toEqual({
      VolumePath: PATH,
      TargetKind: "file",
      Password: "old secret",
      Keyfiles: JSON.stringify(["D:\\keys\\old.key"]),
      Pim: "500",
    });
  });

  test("the probe never carries a new password", () => {
    const request = buildPasswordProbeRequest(PATH, header());
    expect(request).toEqual({ VolumePath: PATH, TargetKind: "file", Password: "old secret", Keyfiles: "[]" });
    expect(JSON.stringify(request)).not.toContain("new secret");
  });

  test("a change names the header the probe found and only that header's fields", () => {
    expect(buildPasswordChangeRequest(PATH, "hidden", header({ newPim: "700", oldKeyfile: " D:\\keys\\old.key " }))).toEqual({
      VolumePath: PATH,
      TargetKind: "file",
      Target: "hidden",
      OldPassword: "old secret",
      NewPassword: "new secret",
      OldKeyfiles: JSON.stringify(["D:\\keys\\old.key"]),
      NewKeyfiles: "[]",
      NewPim: "700",
    });
    const outer = buildPasswordChangeRequest(PATH, "outer", header({ oldPim: "9" }));
    expect(outer.Target).toBe("outer");
    expect(outer.OldPim).toBe("9");
    expect(Object.keys(outer).some(key => key.includes("Hidden"))).toBe(false);
  });

  test("only the two contract headers are accepted from the probe", () => {
    expect(readProbeHeader({ header: "outer" })).toBe("outer");
    expect(readProbeHeader({ header: "hidden" })).toBe("hidden");
    for (const reply of [{ header: "both" }, { header: "Hidden" }, {}, null, "hidden"]) {
      expect(readProbeHeader(reply)).toBeNull();
    }
  });
});

describe("vault password change errors", () => {
  test("a wrong password says only that, on every vault", () => {
    expect(vaultPasswordChangeErrorMessage(new Error("Pro error: vault_wrong_password"), "probe")).toBe("Wrong password.");
    expect(vaultPasswordChangeErrorMessage("engine_unlock_failed", "probe")).toBe("Wrong password.");
    expect(vaultPasswordChangeErrorMessage("wrong_password", "change")).toBe("Wrong password.");
  });

  test("the decoy password of a noted vault asks for the real password", () => {
    for (const stage of ["probe", "change"] as const) {
      expect(vaultPasswordChangeErrorMessage("service rejected request: real_password_required", stage))
        .toBe("Wrong password. Enter the real password for this vault.");
    }
    expect(vaultPasswordChangeErrorMessage("real_password_required", "decoy")).toContain("first password change was kept");
  });

  test("a wrong decoy password in the follow-up says the first change was kept", () => {
    expect(vaultPasswordChangeErrorMessage("wrong_password", "decoy")).toBe("Wrong password. The first password change was kept.");
  });

  test("a policy refusal names who may change the password instead of pointing back to Vault Permissions", () => {
    const error = new Error("service rejected request: vault_policy_managed");
    expect(vaultPasswordChangeErrorMessage(error, "probe")).toBe("Only this vault's owner, or an account it is shared with for editing, can change its password.");
    expect(isPolicyManagedError(error)).toBe(true);
    expect(isPolicyManagedError("wrong_password")).toBe(false);
  });

  test("a password collision is explained only in the decoy follow-up", () => {
    expect(vaultPasswordChangeErrorMessage("new_password_collides", "decoy")).toBe("The new decoy password must differ from the real password.");
    for (const stage of ["probe", "change"] as const) {
      const message = vaultPasswordChangeErrorMessage("new_password_collides", stage);
      expect(message).not.toContain("decoy");
      expect(message).not.toContain("real");
    }
  });

  test("each file and engine code gets its own plain message", () => {
    const cases: Array<[string, string]> = [
      ["vault_file_not_found", "could not find the vault file"],
      ["vault_access_denied", "Windows did not let this account open the vault file"],
      ["vault_file_in_use", "Another program is using the vault file"],
      ["engine_failed", "the original was kept. Nothing changed."],
      ["volume_mounted", "Dismount the vault first"],
      ["vault_already_mounted", "Dismount the vault first"],
      ["mount_state_unknown", "could not confirm that this vault is dismounted"],
      ["weak_new_password", "too weak"],
      ["vault_broker_rejected", "refused this request"],
      ["vault_caller_access_denied", "change the vault file or read its keyfile"],
    ];
    for (const [code, text] of cases) expect(vaultPasswordChangeErrorMessage(code, "change")).toContain(text);
  });

  test("removed decoy codes and wording are gone", () => {
    for (const code of ["wrong_decoy_password", "wrong_real_password", "wrong_both_passwords", "wrong_header"]) {
      expect(vaultPasswordChangeErrorMessage(code, "change")).toBe("The password was not changed. Check the vault file and try again.");
    }
  });

  test("only a validation failure carries a reason, and only a known one", () => {
    expect(vaultPasswordChangeErrorMessage("vault_validation_failed (the selected container could not be found)", "change"))
      .toContain("(Reason: the vault file could not be found or read.)");
    expect(vaultPasswordChangeErrorMessage("vault_file_not_found (the selected container could not be found)", "change")).not.toContain("Reason");
    expect(vaultPasswordChangeErrorMessage("vault_validation_failed (C:\\secret\\path)", "change")).not.toContain("secret");
  });

  test("an unknown result never claims success or repeats raw text", () => {
    const message = vaultPasswordChangeErrorMessage(new Error("C:\\private\\vault.hc secret-value"), "change");
    expect(message).toBe("The password was not changed. Check the vault file and try again.");
  });

  test("a lost change result warns it may have happened; a lost probe changed nothing", () => {
    expect(vaultPasswordChangeErrorMessage("vault_request_timeout", "change")).toContain("may already have changed");
    expect(vaultPasswordChangeErrorMessage("vault_operation_unconfirmed", "decoy")).toContain("may already have changed");
    expect(vaultPasswordChangeErrorMessage("vault_request_timeout", "probe")).toContain("Nothing was changed");
  });
});
