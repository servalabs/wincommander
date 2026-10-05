import { expect, test } from "bun:test";
import { personalVaultSyncError, personalVaultSyncSetupMessage } from "./personalVaultSyncFeedback";

test("recreated sync configuration explains the paused recovery without claiming restored files", () => {
  const message = personalVaultSyncSetupMessage("V:\\Sync", true);
  expect(message).toContain("left paused");
  expect(message).toContain("sharing settings");
  expect(message).toContain("Deleted files have not been restored");
  expect(personalVaultSyncSetupMessage("V:\\Sync", false)).toContain("Connect your other device");
});

test("a responding helper's rejected folder is not reported as unreachable", () => {
  expect(personalVaultSyncError(new Error("vault_broker_rejected"))).toContain("helper responded");
  expect(personalVaultSyncError("vault_syncthing_root_conflict")).toContain("current location");
  expect(personalVaultSyncError("vault_broker_unavailable")).toContain("could not start the sync helper");
});

test("failed installation, retained configuration and unknown completion need different next steps", () => {
  expect(personalVaultSyncError("vault_syncthing_install_failed")).toContain("download could not be verified");
  expect(personalVaultSyncError("vault_syncthing_profile_unavailable")).toContain("identity has been preserved");
  expect(personalVaultSyncError("vault_request_timeout")).toContain("before submitting another request");
  expect(personalVaultSyncError("service operation did not confirm before its deadline; refresh vault state before retrying")).toContain("before submitting another request");
  expect(personalVaultSyncError("private path C:\\Users\\secret; token=private")).not.toContain("secret");
});
