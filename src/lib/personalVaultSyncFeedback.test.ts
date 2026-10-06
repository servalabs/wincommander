import { expect, test } from "bun:test";
import { personalVaultSyncError, personalVaultSyncSetupMessage, validatePersonalVaultSyncFolders } from "./personalVaultSyncFeedback";

test("confirmed new sync setup explains the remaining device connection step", () => {
  expect(personalVaultSyncSetupMessage("V:\\Sync")).toContain("Connect your other device");
  expect(personalVaultSyncSetupMessage("V:\\Sync")).toContain("does not stop Syncthing");
});

test("sync setup accepts separate child folders but refuses duplicate or nested folders", () => {
  expect(validatePersonalVaultSyncFolders(["Phone/Camera", "Phone\\Documents"])).toEqual({ ok: true, folders: ["Phone\\Camera", "Phone\\Documents"] });
  expect(validatePersonalVaultSyncFolders(["Phone", "Phone/Camera"]).ok).toBe(false);
  expect(validatePersonalVaultSyncFolders(["Phone/Camera", "phone\\camera"]).ok).toBe(false);
  expect(validatePersonalVaultSyncFolders(["../Camera"]).ok).toBe(false);
});

test("reopening recreated sync preserves phone sharing and existing-file guidance", () => {
  const message = personalVaultSyncSetupMessage("V:\\Sync", true);
  expect(message).toContain("Share this new folder");
  expect(message).toContain("old Syncthing folder entry while keeping the files");
  expect(message).toContain("accept the new share");
  expect(message).not.toContain("Connect your other device");
});

test("a responding helper's rejected folder is not reported as unreachable", () => {
  expect(personalVaultSyncError(new Error("vault_broker_rejected"))).toContain("helper responded");
  expect(personalVaultSyncError("vault_syncthing_root_conflict")).toContain("current location");
  expect(personalVaultSyncError("vault_broker_unavailable")).toContain("could not start the sync helper");
});

test("failed installation, retained configuration and unknown completion need different next steps", () => {
  expect(personalVaultSyncError("vault_syncthing_not_enabled")).toContain("turn on Syncthing");
  expect(personalVaultSyncError("vault_syncthing_install_failed")).toContain("download could not be verified");
  expect(personalVaultSyncError("vault_syncthing_profile_unavailable")).toContain("identity has been preserved");
  expect(personalVaultSyncError("vault_request_timeout")).toContain("before submitting another request");
  expect(personalVaultSyncError("service operation did not confirm before its deadline; refresh vault state before retrying")).toContain("before submitting another request");
  expect(personalVaultSyncError("private path C:\\Users\\secret; token=private")).not.toContain("secret");
});
