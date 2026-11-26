import { describe, expect, test } from "bun:test";
import {
  EMPTY_HEADER_CREDENTIALS,
  EMPTY_PASSWORD_CHANGE_FORM,
  buildPasswordChangeRequest,
  passwordChangeFormProblem,
  vaultPasswordChangeErrorMessage,
  withoutPasswords,
  type HeaderCredentials,
  type VaultPasswordChangeForm,
} from "./vaultPasswordChange";

const header = (oldPassword: string, newPassword: string, changes: Partial<HeaderCredentials> = {}): HeaderCredentials => ({
  ...EMPTY_HEADER_CREDENTIALS,
  oldPassword,
  newPassword,
  confirmPassword: newPassword,
  ...changes,
});

const form = (changes: Partial<HeaderCredentials> = {}): VaultPasswordChangeForm => ({
  ...EMPTY_PASSWORD_CHANGE_FORM,
  volumePath: "D:\\Vaults\\notes.hc",
  outer: header("old secret", "new secret", changes),
});

const decoyForm = (decoy: Partial<HeaderCredentials> = {}, real: Partial<HeaderCredentials> = {}): VaultPasswordChangeForm => ({
  volumePath: "D:\\Vaults\\notes.hc",
  vaultKind: "decoy",
  outer: header("old decoy", "new decoy", decoy),
  hidden: header("old real", "new real", real),
});

describe("standard vault password change form", () => {
  test("a complete form has no problem", () => {
    expect(passwordChangeFormProblem(form())).toBeNull();
  });

  test("requires a vault file, the current password and a new password", () => {
    expect(passwordChangeFormProblem({ ...form(), volumePath: "  " })).toBe("Choose the vault file.");
    expect(passwordChangeFormProblem(form({ oldPassword: "" }))).toBe("Enter the current password.");
    expect(passwordChangeFormProblem(form({ newPassword: "", confirmPassword: "" }))).toBe("Enter a new password.");
  });

  test("an empty password is allowed only when a keyfile supplies that credential", () => {
    expect(passwordChangeFormProblem(form({ oldPassword: "", oldKeyfile: "D:\\keys\\old.key" }))).toBeNull();
    expect(passwordChangeFormProblem(form({ newPassword: "", confirmPassword: "", newKeyfile: "D:\\keys\\new.key" }))).toBeNull();
  });

  test("the new password must be typed the same way twice", () => {
    expect(passwordChangeFormProblem(form({ confirmPassword: "new secreT" }))).toBe("The new passwords do not match.");
  });

  test("the new credential must differ from the current one", () => {
    const same = form({ newPassword: "old secret", confirmPassword: "old secret" });
    expect(passwordChangeFormProblem(same)).toBe("The new password must be different from the current one.");
    expect(passwordChangeFormProblem({ ...same, outer: { ...same.outer, newPim: "600" } })).toBeNull();
    expect(passwordChangeFormProblem({ ...same, outer: { ...same.outer, newKeyfile: "D:\\keys\\new.key" } })).toBeNull();
  });

  test("rejects a PIM outside the engine's range", () => {
    for (const pim of ["0", "2147469", "12a", "-5", "1.5"]) {
      expect(passwordChangeFormProblem(form({ newPim: pim }))).toContain("PIM");
      expect(passwordChangeFormProblem(form({ oldPim: pim }))).toContain("PIM");
    }
    expect(passwordChangeFormProblem(form({ oldPim: "1", newPim: "2147468" }))).toBeNull();
  });

  test("leftover real-password fields never block or leak into a standard change", () => {
    const leftover = { ...form(), hidden: header("", "x", { confirmPassword: "y" }) };
    expect(passwordChangeFormProblem(leftover)).toBeNull();
    expect(Object.keys(buildPasswordChangeRequest(leftover)).some(key => key.includes("Hidden"))).toBe(false);
  });
});

describe("decoy vault password change form", () => {
  test("a complete form has no problem", () => {
    expect(passwordChangeFormProblem(decoyForm())).toBeNull();
  });

  test("asks for all four passwords", () => {
    expect(passwordChangeFormProblem(decoyForm({ oldPassword: "" }))).toBe("Enter the current decoy password.");
    expect(passwordChangeFormProblem(decoyForm({}, { oldPassword: "" }))).toBe("Enter the current real password.");
    expect(passwordChangeFormProblem(decoyForm({ newPassword: "", confirmPassword: "" }))).toBe("Enter a new decoy password.");
    expect(passwordChangeFormProblem(decoyForm({}, { newPassword: "", confirmPassword: "" }))).toBe("Enter a new real password.");
  });

  test("each new password must be confirmed", () => {
    expect(passwordChangeFormProblem(decoyForm({ confirmPassword: "new decoY" }))).toBe("The new decoy passwords do not match.");
    expect(passwordChangeFormProblem(decoyForm({}, { confirmPassword: "new reaL" }))).toBe("The new real passwords do not match.");
  });

  test("the new decoy and real passwords must differ", () => {
    const same = decoyForm({}, { newPassword: "new decoy", confirmPassword: "new decoy" });
    expect(passwordChangeFormProblem(same)).toBe("The new decoy and real passwords must be different.");
    expect(passwordChangeFormProblem({ ...same, hidden: { ...same.hidden, newKeyfile: "D:\\keys\\real.key" } })).toBeNull();
  });

  test("one password may stay the same, but not both", () => {
    expect(passwordChangeFormProblem(decoyForm({ newPassword: "old decoy", confirmPassword: "old decoy" }))).toBeNull();
    expect(passwordChangeFormProblem(decoyForm(
      { newPassword: "old decoy", confirmPassword: "old decoy" },
      { newPassword: "old real", confirmPassword: "old real" },
    ))).toBe("Change at least one of the two passwords.");
  });

  test("rejects a bad real-password PIM", () => {
    expect(passwordChangeFormProblem(decoyForm({}, { newPim: "0" }))).toContain("PIM");
  });
});

describe("vault password change request", () => {
  test("a standard vault changes its only header", () => {
    expect(buildPasswordChangeRequest(form({ newPim: "700", oldKeyfile: " D:\\keys\\old.key " }))).toEqual({
      VolumePath: "D:\\Vaults\\notes.hc",
      TargetKind: "file",
      Target: "outer",
      OldPassword: "old secret",
      NewPassword: "new secret",
      OldKeyfiles: JSON.stringify(["D:\\keys\\old.key"]),
      NewKeyfiles: "[]",
      NewPim: "700",
    });
  });

  test("a decoy vault sends both headers in one request", () => {
    const request = buildPasswordChangeRequest(decoyForm(
      { oldPim: "500" },
      { newKeyfile: " D:\\keys\\real.key ", oldPim: "900", newPim: "901" },
    ));
    expect(request).toEqual({
      VolumePath: "D:\\Vaults\\notes.hc",
      TargetKind: "file",
      Target: "both",
      OldPassword: "old decoy",
      NewPassword: "new decoy",
      OldKeyfiles: "[]",
      NewKeyfiles: "[]",
      OldPim: "500",
      OldHiddenPassword: "old real",
      NewHiddenPassword: "new real",
      OldHiddenKeyfiles: "[]",
      NewHiddenKeyfiles: JSON.stringify(["D:\\keys\\real.key"]),
      OldHiddenPim: "900",
      NewHiddenPim: "901",
    });
  });

  test("clearing passwords keeps the path, type, PIMs and keyfiles", () => {
    const cleared = withoutPasswords(decoyForm({ newKeyfile: "D:\\k" }, { oldPim: "9" }));
    for (const part of [cleared.outer, cleared.hidden]) {
      expect([part.oldPassword, part.newPassword, part.confirmPassword]).toEqual(["", "", ""]);
    }
    expect(cleared.vaultKind).toBe("decoy");
    expect(cleared.outer.newKeyfile).toBe("D:\\k");
    expect(cleared.hidden.oldPim).toBe("9");
  });
});

describe("vault password change errors", () => {
  test("each contract code gets its own plain message", () => {
    expect(vaultPasswordChangeErrorMessage(new Error("wrong_password"))).toContain("did not open this vault");
    expect(vaultPasswordChangeErrorMessage("new_password_collides")).toContain("other password of this decoy vault");
    expect(vaultPasswordChangeErrorMessage("weak_new_password")).toContain("too weak");
    expect(vaultPasswordChangeErrorMessage("volume_mounted")).toContain("Dismount the vault first");
    expect(vaultPasswordChangeErrorMessage("engine_failed")).toContain("could not change the password");
    expect(vaultPasswordChangeErrorMessage("vault_broker_rejected")).toContain("refused this request");
  });

  test("a decoy change names which current password was wrong", () => {
    expect(vaultPasswordChangeErrorMessage("wrong_decoy_password")).toBe("The current decoy password is wrong. Nothing was changed.");
    expect(vaultPasswordChangeErrorMessage("vault_wrong_real_password")).toBe("The current real password is wrong, or this vault has only one password. Nothing was changed.");
    expect(vaultPasswordChangeErrorMessage(new Error("Pro error: wrong_both_passwords"))).toBe("Both current passwords are wrong. Nothing was changed.");
  });

  test("the real password of a decoy vault in the one-password form points to the decoy type", () => {
    expect(vaultPasswordChangeErrorMessage("wrong_header")).toBe("This password is the real password of a decoy vault. Choose \"Decoy vault\" and enter both passwords. Nothing was changed.");
    expect(vaultPasswordChangeErrorMessage("Pro error: vault_wrong_header")).toContain("Choose \"Decoy vault\"");
  });

  test("prefixed and wrapped service codes are recognised", () => {
    expect(vaultPasswordChangeErrorMessage("vault_already_mounted")).toContain("Dismount the vault first");
  });

  test("an unknown result never claims success or repeats raw text", () => {
    const message = vaultPasswordChangeErrorMessage(new Error("C:\\private\\vault.hc secret-value"));
    expect(message).toBe("The password was not changed. Check the vault file and try again.");
    expect(message).not.toContain("secret-value");
  });

  test("a lost result warns that the change may already have happened", () => {
    expect(vaultPasswordChangeErrorMessage("vault_request_timeout")).toContain("may already have changed");
    expect(vaultPasswordChangeErrorMessage("vault_operation_unconfirmed")).toContain("may already have changed");
  });
});
