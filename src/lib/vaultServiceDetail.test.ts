import { describe, expect, test } from "bun:test";
import { cleanVaultPath, findVaultName, vaultRenameErrorMessage } from "./vaultRename";
import { buildPasswordChangeRequest, EMPTY_PASSWORD_CHANGE_FORM, vaultPasswordChangeErrorMessage } from "./vaultPasswordChange";

describe("vault paths copied from Explorer", () => {
  test("one pair of surrounding quotes is removed", () => {
    expect(cleanVaultPath('  "D:\\Vault\\New folder\\test3"  ')).toBe("D:\\Vault\\New folder\\test3");
    expect(cleanVaultPath("D:\\Vault\\test3")).toBe("D:\\Vault\\test3");
    expect(cleanVaultPath('"D:\\a"b"')).toBe('"D:\\a"b"');
  });

  test("a quoted path finds the name the service stored", () => {
    const names = [{ path: "\\\\?\\D:\\Vault\\New folder\\test3", name: "Test33" }];
    expect(findVaultName(names, '"D:\\Vault\\New folder\\test3"')).toBe("Test33");
  });

  test("the password change request sends the path without quotes", () => {
    const request = buildPasswordChangeRequest({ ...EMPTY_PASSWORD_CHANGE_FORM, volumePath: '"D:\\Vault\\test3"' });
    expect(request.VolumePath).toBe("D:\\Vault\\test3");
  });
});

describe("validation failures name a known reason only", () => {
  test("a known service reason is shown in plain words", () => {
    expect(vaultRenameErrorMessage(new Error("service rejected request: vault_validation_failed (rename request is invalid) [operation 4]")))
      .toBe("Check the vault file and the name, then try again. Nothing was changed. (Reason: the request was not accepted.)");
    expect(vaultPasswordChangeErrorMessage("vault_validation_failed (the selected container could not be found)"))
      .toBe("Check the vault file and the passwords, then try again. Nothing was changed. (Reason: the vault file could not be found or read.)");
  });

  test("unknown text is never echoed", () => {
    expect(vaultRenameErrorMessage("vault_validation_failed (C:\\secret\\path)"))
      .toBe("Check the vault file and the name, then try again. Nothing was changed.");
  });

  test("rename maps a write refusal", () => {
    expect(vaultRenameErrorMessage("vault_caller_access_denied")).toContain("did not let this account write the vault file");
  });
});
