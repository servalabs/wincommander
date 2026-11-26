import { describe, expect, test } from "bun:test";
import {
  buildRenameRequest,
  newFileNameProblem,
  readRenameResult,
  renameProblem,
  renameReducer,
  renamedFileName,
  startRename,
  vaultRenameErrorMessage,
  type RenameEvent,
  type RenameFlow,
} from "./vaultFileRename";

const PATH = "D:\\Vaults\\notes.hc";
const run = (flow: RenameFlow, ...events: RenameEvent[]) => events.reduce(renameReducer, flow);
const ready = () => run(startRename(PATH), { type: "name", newName: "taxes" }, { type: "edit", field: "oldPassword", value: "secret" });

describe("new file name", () => {
  test("starts from the current file name, which cannot be sent unchanged", () => {
    expect(startRename(`"${PATH}"`).newName).toBe("notes.hc");
    expect(newFileNameProblem("notes.hc", PATH)).toContain("already the file's name");
    expect(newFileNameProblem("NOTES", PATH)).toContain("already the file's name");
  });

  test("keeps the old extension when the new name has none", () => {
    expect(renamedFileName("taxes", "notes.hc")).toBe("taxes.hc");
    expect(renamedFileName("taxes.tc", "notes.hc")).toBe("taxes.tc");
    expect(renamedFileName("taxes", "notes")).toBe("taxes");
  });

  test("refuses names Windows would refuse", () => {
    for (const name of ["a/b", "a\\b", "a:b", "a*b", "a?b", 'a"b', "a<b", "a>b", "a|b", "tab\there"]) {
      expect(newFileNameProblem(name, PATH)).toContain("cannot contain");
    }
    for (const name of [".", "..", "taxes.", "taxes "]) expect(newFileNameProblem(name, PATH)).toContain("cannot end");
    for (const name of ["CON", "nul.hc", "com1", "LPT9.txt"]) expect(newFileNameProblem(name, PATH)).toContain("reserves");
    expect(newFileNameProblem("a".repeat(253), PATH)).toContain("too long");
    expect(newFileNameProblem("", PATH)).toBe("Enter the new file name.");
    expect(newFileNameProblem("console.hc", PATH)).toBeNull();
  });

  test("a name the person has not edited follows a newly chosen file", () => {
    expect(run(startRename(""), { type: "path", volumePath: "E:\\x\\work.hc" }).newName).toBe("work.hc");
    expect(run(ready(), { type: "path", volumePath: "E:\\x\\work.hc" }).newName).toBe("taxes");
  });
});

describe("rename request", () => {
  test("needs a file, a new name and the current password", () => {
    expect(renameProblem(startRename())).toBe("Choose the vault file.");
    expect(renameProblem(run(startRename(PATH), { type: "name", newName: "taxes" }))).toBe("Enter the current password.");
    expect(renameProblem(ready())).toBeNull();
    expect(renameProblem(run(ready(), { type: "edit", field: "oldPim", value: "0" }))).toContain("PIM");
  });

  test("carries the secrets only in the request, never in state after sending", () => {
    const typed = run(ready(), { type: "edit", field: "oldPim", value: "485" }, { type: "edit", field: "oldKeyfile", value: " D:\\key.bin " });
    expect(buildRenameRequest(`"${PATH}"`, typed.newName, typed.credentials)).toEqual({
      containerPath: PATH, newName: "taxes", password: "secret", pim: "485", keyfiles: ["D:\\key.bin"],
    });
    const sent = run(typed, { type: "send" });
    expect(sent.busy).toBe(true);
    expect(sent.credentials.oldPassword).toBe("");
    expect(JSON.stringify(run(sent, { type: "failed", error: "wrong_password" }))).not.toContain("secret");
    const done = run(sent, { type: "renamed", result: { path: "D:\\Vaults\\taxes.hc", name: "taxes.hc" } });
    expect(done.credentials).toEqual(startRename().credentials);
    expect(done.volumePath).toBe("D:\\Vaults\\taxes.hc");
    expect(done.renamed).toBe("taxes.hc");
  });

  test("a password-only vault sends no PIM and no keyfiles", () => {
    const request = buildRenameRequest(PATH, "taxes", ready().credentials);
    expect(request.pim).toBeNull();
    expect(request.keyfiles).toEqual([]);
  });

  test("only the contract's success reply counts as renamed", () => {
    expect(readRenameResult({ status: "renamed", path: "D:\\Vaults\\taxes.hc", name: "taxes.hc" })).toEqual({ path: "D:\\Vaults\\taxes.hc", name: "taxes.hc" });
    for (const reply of [{ status: "renamed", name: "taxes.hc" }, { status: "renamed", path: "", name: "x" }, { status: "ok", path: "a", name: "b" }, null, "renamed"]) {
      expect(readRenameResult(reply)).toBeNull();
    }
  });

  test("closing clears everything", () => {
    expect(run(ready(), { type: "reset" })).toEqual(startRename());
  });
});

describe("vaultRenameErrorMessage", () => {
  test("explains each contract code in plain words", () => {
    const cases: Array<[string, string]> = [
      ["service rejected request: real_password_required", "Wrong password. Enter the real password for this vault."],
      ["wrong_password", "Wrong password."],
      ["vault_policy_managed", "Only this vault's owner, or an account it is shared with for editing, can rename it."],
      ["name_invalid", "Windows does not allow that file name. Avoid \\ / : * ? \" < > |, a dot or space at the end, and names such as CON or NUL. Nothing was changed."],
      ["name_exists", "A file with that name already exists in this folder. Choose another name. Nothing was changed."],
      ["name_unchanged", "This is already the file's name. Enter a different one."],
      ["volume_mounted", "Dismount the vault first, then rename it. Nothing was changed."],
      ["vault_rename_not_saved", "WinCommander could not update its records for the new name, so the file kept its old name. Nothing was changed. Try again."],
    ];
    for (const [error, expected] of cases) expect(vaultRenameErrorMessage(new Error(error))).toBe(expected);
  });

  test("the remaining codes each get their own message", () => {
    const cases: Array<[string, string]> = [
      ["vault_mount_state_unknown", "could not confirm"],
      ["vault_access_denied", "did not let this account rename"],
      ["vault_file_not_found", "could not find the vault file"],
      ["vault_file_in_use", "Another program"],
      ["engine_failed", "encryption engine"],
      ["vault_session_unavailable", "signed-in Windows desktop session"],
      ["vault_validation_failed", "Check the vault file and the name"],
      ["service operation did not confirm before its deadline", "did not get a result in time"],
      ["pro_not_installed", "up-to-date WinCommander Pro"],
    ];
    for (const [error, expected] of cases) expect(vaultRenameErrorMessage(error)).toContain(expected);
  });

  test("never echoes unknown error text, and never points back to Vault Permissions", () => {
    expect(vaultRenameErrorMessage(new Error("C:\\secret\\path failed"))).toBe("The vault was not renamed. Check the vault file and try again.");
    expect(vaultRenameErrorMessage("vault_policy_managed")).not.toContain("Vault Permissions");
  });
});
