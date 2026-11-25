import { describe, expect, test } from "bun:test";
import {
  EMPTY_PASSWORD_CHANGE_FORM,
  buildPasswordChangeRequest,
  passwordChangeFormProblem,
  passwordChangeTarget,
  vaultPasswordChangeErrorMessage,
  type VaultPasswordChangeForm,
} from "./vaultPasswordChange";

const form = (changes: Partial<VaultPasswordChangeForm> = {}): VaultPasswordChangeForm => ({
  ...EMPTY_PASSWORD_CHANGE_FORM,
  volumePath: "D:\\Vaults\\notes.hc",
  oldPassword: "old secret",
  newPassword: "new secret",
  confirmPassword: "new secret",
  ...changes,
});

describe("vault password change form", () => {
  test("a complete form has no problem", () => {
    expect(passwordChangeFormProblem(form())).toBeNull();
  });

  test("requires a vault file, the current password and a new password", () => {
    expect(passwordChangeFormProblem(form({ volumePath: "  " }))).toBe("Choose the vault file.");
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
    expect(passwordChangeFormProblem({ ...same, newPim: "600" })).toBeNull();
    expect(passwordChangeFormProblem({ ...same, newKeyfile: "D:\\keys\\new.key" })).toBeNull();
  });

  test("rejects a PIM outside the engine's range", () => {
    for (const pim of ["0", "2147469", "12a", "-5", "1.5"]) {
      expect(passwordChangeFormProblem(form({ newPim: pim }))).toContain("PIM");
      expect(passwordChangeFormProblem(form({ oldPim: pim }))).toContain("PIM");
    }
    expect(passwordChangeFormProblem(form({ oldPim: "1", newPim: "2147468" }))).toBeNull();
  });
});

describe("vault password change target", () => {
  test("a single-password vault always changes its only password", () => {
    expect(passwordChangeTarget({ vaultKind: "standard", which: "real" })).toBe("outer");
  });

  test("a decoy vault changes only the chosen password", () => {
    expect(passwordChangeTarget({ vaultKind: "decoy", which: "decoy" })).toBe("outer");
    expect(passwordChangeTarget({ vaultKind: "decoy", which: "real" })).toBe("hidden");
  });

  test("the request carries the chosen header, credentials and optional PIMs", () => {
    const request = buildPasswordChangeRequest(form({ vaultKind: "decoy", which: "real", newPim: "700", oldKeyfile: " D:\\keys\\old.key " }));
    expect(request).toEqual({
      VolumePath: "D:\\Vaults\\notes.hc",
      TargetKind: "file",
      Target: "hidden",
      OldPassword: "old secret",
      NewPassword: "new secret",
      OldKeyfiles: JSON.stringify(["D:\\keys\\old.key"]),
      NewKeyfiles: "[]",
      NewPim: "700",
    });
  });
});

describe("vault password change errors", () => {
  test("each contract code gets its own plain message", () => {
    expect(vaultPasswordChangeErrorMessage("wrong_header")).toContain("That password belongs to the other password of this decoy vault");
    expect(vaultPasswordChangeErrorMessage(new Error("wrong_password"))).toContain("did not open this vault");
    expect(vaultPasswordChangeErrorMessage("new_password_collides")).toContain("other password of this decoy vault");
    expect(vaultPasswordChangeErrorMessage("weak_new_password")).toContain("too weak");
    expect(vaultPasswordChangeErrorMessage("volume_mounted")).toContain("Dismount the vault first");
    expect(vaultPasswordChangeErrorMessage("engine_failed")).toContain("could not change the password");
    expect(vaultPasswordChangeErrorMessage("vault_broker_rejected")).toContain("refused this request");
  });

  test("prefixed and wrapped service codes are recognised", () => {
    expect(vaultPasswordChangeErrorMessage("Pro error: vault_wrong_header")).toContain("That password belongs");
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
