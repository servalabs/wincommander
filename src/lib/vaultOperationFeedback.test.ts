import { expect, test } from "bun:test";
import { confirmedBulkDismountMessage, confirmedMountObservationError, isAuthorizedBulkDismountReceipt, selectableDriveLetters, vaultOperationError } from "./vaultOperationFeedback";
import type { EncryptionStatus } from "@/hooks/useBackend";

test("pre-request service failures identify their stage without restart advice or raw context", () => {
  for (const [code, stage] of [
    ["vault_service_unavailable", "connection"],
    ["vault_service_identity_rejected", "identity"],
    ["vault_service_handshake_rejected", "handshake"],
    ["vault_service_handshake_timeout", "handshake"],
  ]) {
    const message = vaultOperationError(`${code} secret-value C:\\protected\\container.hc`);
    expect(message.toLowerCase()).toContain(stage);
    expect(message).not.toContain("secret-value");
    expect(message).not.toContain("protected");
    expect(message.toLowerCase()).not.toContain("restart");
    expect(message).toContain("No mount or dismount was started");
  }
});

test("bulk success requires an explicit bounded authorized-subset receipt", () => {
  const receipt = { status: "authorized_dismounted", state: "unmounted", scope: "authorized", dismounted: 2 };
  expect(isAuthorizedBulkDismountReceipt(receipt)).toBe(true);
  expect(isAuthorizedBulkDismountReceipt({ ...receipt, dismounted: 0 })).toBe(true);
  for (const value of [null, {}, { status: "ok" }, { ...receipt, scope: "all" }, { ...receipt, state: "mounted" },
    { ...receipt, dismounted: -1 }, { ...receipt, dismounted: 27 }, { ...receipt, dismounted: 1.5 }, { ...receipt, dismounted: "2" }]) {
    expect(isAuthorizedBulkDismountReceipt(value)).toBe(false);
  }
});

test("a verified dismount receipt is not reversed by a superseded or failed list refresh", () => {
  const receipt = { status: "authorized_dismounted", state: "unmounted", scope: "authorized", dismounted: 1 };
  expect(confirmedBulkDismountMessage(receipt, null)).toContain("1 encrypted volume(s) dismounted");
  expect(confirmedBulkDismountMessage(receipt, { volumes: [{ letter: "J:", dismountAllowed: true }] } as unknown as EncryptionStatus))
    .toContain("1 encrypted volume(s) dismounted");
  let failure = "";
  try { confirmedBulkDismountMessage({ status: "ok" }, { volumes: [] } as unknown as EncryptionStatus); }
  catch (error) { failure = (error as Error).message; }
  expect(failure).toBe("vault_dismount_readback_unconfirmed");
});

test("shows the same unlock guidance for native string and Error rejections", () => {
  const message = vaultOperationError("vault_engine_unlock_failed");
  expect(message).toContain("password, PIM and keyfiles");
  expect(vaultOperationError(new Error("vault_engine_unlock_failed"))).toBe(message);
});

test("failed mount rollback reports uncertain cleanup without claiming dismount success", () => {
  for (const code of ["vault_cleanup_failed", "vault_dismount_failed", "dismount_failed"]) {
    const message = vaultOperationError(code, "mount");
    expect(message).toContain("cleanup could not be confirmed");
    expect(message).toContain("may still be mounted");
    expect(message).not.toContain("No mount");
    expect(message).not.toContain("password");
  }
  expect(vaultOperationError("vault_dismount_failed", "dismount")).not.toContain("Mounting failed");
});

test("dismount cleanup uncertainty explains the mapping stage without mount wording or raw details", () => {
  const message = vaultOperationError("vault_cleanup_failed C:\\private\\test.hc password=hidden", "dismount");
  expect(message).toContain("Dismount cleanup could not be confirmed");
  expect(message).toContain("mapping may still be in use");
  expect(message).not.toContain("Mounting failed");
  expect(message).not.toContain("private");
  expect(message).not.toContain("hidden");
});

test("runtime incompatibility distinguishes preflight denial from unverified post-operation status", () => {
  const preflight = vaultOperationError("vault_runtime_update_required");
  expect(preflight).toContain("app, Pro component and Vault service");
  expect(preflight).toContain("Update or repair them together");
  expect(preflight).toContain("matching installer and Pro release");
  expect(preflight).toContain("Reinstalling an older component will not fix this");
  expect(preflight).toContain("No mount or dismount was started");
  const status = vaultOperationError("vault_service_personal_status_invalid", "dismount");
  expect(status).toContain("app, Pro component and Vault service");
  expect(status).toContain("Update or repair them together");
  expect(status).toContain("could not be verified");
  expect(status).not.toContain("No mount or dismount was started");
  expect(status).not.toContain("password");
});

test("confirmed mounts distinguish unavailable inventory from an observed missing or replaced drive", () => {
  const mount = { drive: "J:", internalDrive: 4 };
  const observation = (volumes: unknown[]) => ({ volumes }) as EncryptionStatus;
  const unavailable = confirmedMountObservationError(mount, null);
  const missing = confirmedMountObservationError(mount, observation([]));
  expect(unavailable).toBe("vault_confirmed_mount_list_unavailable");
  expect(missing).toBe("vault_confirmed_mount_not_in_list");
  expect(confirmedMountObservationError(mount, observation([{ letter: "J:", internalDrive: 5 }]))).toBe(missing);
  expect(confirmedMountObservationError(mount, observation([{ ...mount, letter: "J:", accessible: false }]))).toBe(missing);
  expect(confirmedMountObservationError(mount, observation([{ ...mount, letter: "J:", accessible: true }]))).toBe(null);
  expect(vaultOperationError(unavailable)).toContain("mount and Windows drive access were confirmed");
  expect(vaultOperationError(unavailable)).toContain("does not mean mounting failed");
  expect(vaultOperationError(missing)).toContain("latest volume list no longer shows");
  expect(vaultOperationError(unavailable)).not.toContain("password");
});

test("missing Pro installation and licensing failure never diagnose the container password", () => {
  for (const error of ["PRO_NOT_INSTALLED: missing engine", "vault_pro_not_installed"]) {
    expect(vaultOperationError(error)).toContain("Pro module is not installed");
    expect(vaultOperationError(error)).not.toContain("password");
  }
  expect(vaultOperationError("vault_entitlement_denied")).toContain("verify or activate your key");
  expect(vaultOperationError("vault_entitlement_denied")).not.toContain("password");
});

test("timeouts and interrupted operations never claim failure or encourage automatic replay", () => {
  for (const operation of ["mount", "dismount"] as const) {
    expect(vaultOperationError("vault_request_timeout", operation)).toContain("may still be running");
    expect(vaultOperationError("vault_operation_unconfirmed", operation)).toContain("may have completed");
    expect(vaultOperationError("vault_operation_unconfirmed", operation)).not.toContain("password");
  }
  expect(vaultOperationError("vault_mount_options_timeout")).toContain("No mount was started");
  expect(vaultOperationError("All pipe instances are busy. (os error 231)")).toContain("service is busy");
});

test("generic engine failures never diagnose an incorrect password or expose transport data", () => {
  expect(vaultOperationError("vault_engine_mount_failed")).not.toContain("password");
  expect(vaultOperationError("transport failed C:\\private\\secret.hc secret=example")).not.toContain("secret");
});

test("busy letters and Windows access denial have actionable messages", () => {
  expect(vaultOperationError("vault_engine_drive_letter_unavailable")).toContain("reserved");
  expect(vaultOperationError("vault_caller_access_denied")).toContain("automatically repairs");
  expect(vaultOperationError("unknown", "dismount")).toContain("Close files");
});

test("mounted-root access denial explains automatic ordinary-Vault recovery", () => {
  const message = vaultOperationError("vault_caller_access_denied");
  expect(message).toContain("inside the mounted Vault");
  expect(message).toContain("automatically repairs");
  expect(message).toContain("Fleet-managed Vault");
  expect(message).toContain("Fleet > Vault permissions");
  expect(message).not.toContain("take ownership");
  expect(vaultOperationError(new Error("vault_caller_access_denied"))).toBe(message);
});

test("registered ownership and Fleet denial give policy recovery rather than filesystem repair", () => {
  const owner = vaultOperationError("vault_private_owner_required", "dismount");
  expect(owner).toContain("owner's Windows account");
  expect(owner).toContain("dismount it first");
  expect(owner).toContain("authorized administrator");
  expect(owner).toContain("Fleet > Vault permissions");
  const fleet = vaultOperationError("vault_policy_access_denied");
  expect(fleet).toContain("Fleet > Vault permissions");
  for (const message of [owner, fleet]) {
    expect(message).not.toContain("Properties > Security");
    expect(message).not.toContain("take ownership");
    expect(message).not.toContain("password");
  }
});

test("private-owner denial names the authorized account without requiring the original logon", () => {
  for (const operation of ["mount", "dismount"] as const) {
    const message = vaultOperationError("vault_private_owner_required S-1-5-private-owner C:\\protected\\vault.hc", operation);
    expect(message).toContain("registered owner");
    expect(message).toContain("owner's Windows account");
    expect(message).toContain("administrator access does not replace ownership");
    expect(message).not.toContain("original mounting session");
    expect(message).not.toContain("session where it was mounted");
    expect(message).not.toContain("S-1-5-private-owner");
    expect(message).not.toContain("protected");
  }
});

test("ambiguous Windows access denial does not invent which filesystem target failed", () => {
  const message = vaultOperationError("Access is denied: C:\\private\\confidential.hc");
  expect(message).not.toContain("confidential");
  expect(message).not.toContain("container");
  expect(message).toContain("item's owner");
  expect(message).not.toContain("mounted drive in File Explorer");
  expect(message).not.toContain("Its saved permissions were not changed");
});

test("administration, Fleet policy, private ownership and unknown state remain distinct", () => {
  const admin = vaultOperationError("vault_administrator_required", "dismount");
  const fleet = vaultOperationError("vault_policy_access_denied", "dismount");
  const owner = vaultOperationError("vault_private_owner_required", "dismount");
  const unknown = vaultOperationError("vault_mount_state_unknown", "dismount");
  expect(admin).toContain("administrator approval");
  expect(fleet).toContain("Fleet Vault permission");
  expect(owner).toContain("administrator access does not replace ownership");
  expect(unknown).toContain("could not be verified");
  expect(unknown).toContain("original mounting tool");
  expect(unknown).toContain("restart the WinCommander service");
  for (const message of [admin, fleet, owner, unknown]) expect(message).not.toContain("password");
});

test("already-mounted requests direct users to the existing drive without diagnosing credentials", () => {
  expect(vaultOperationError("vault_already_mounted")).toContain("Open the existing drive");
  expect(vaultOperationError("vault_already_mounted")).not.toContain("password");
});

test("availability is never invented and other draft reservations are excluded", () => {
  expect(selectableDriveLetters([])).toEqual([]);
  expect(selectableDriveLetters(["v", "W:", "W", "Z", "??", "C:\\"], ["V", "z:"])).toEqual(["W"]);
});

test("partial bulk dismount preserves safe counts and its exact authorization category", () => {
  const message = vaultOperationError("vault_bulk_dismount_partial:policy_access_denied:2:1", "dismount");
  expect(message).toContain("2 encrypted volume(s) dismounted; 1 not confirmed dismounted");
  expect(message).toContain("Fleet Vault permission");
  expect(message).not.toContain("password");
  expect(vaultOperationError("vault_bulk_dismount_partial:secret-user:2:1", "dismount")).not.toContain("secret-user");
});

test("Syncthing readiness failure stops dismount at the pause check without exposing private context", () => {
  const message = vaultOperationError("vault_syncthing_profile_unavailable secret-key C:\\protected\\profile", "dismount");
  expect(message).toContain("synced folders are safely paused");
  expect(message).toContain("retry the dismount check");
  expect(message).toContain("not confirmed dismounted");
  expect(message).not.toContain("Keep the personal Vault mounted");
  expect(message).not.toContain("secret-key");
  expect(message).not.toContain("protected");
  expect(vaultOperationError("vault_syncthing_profile_unavailable", "mount")).toContain("Keep the personal Vault mounted");
});

test("partial Syncthing dismount failure keeps confirmed counts and the remaining pause barrier", () => {
  const message = vaultOperationError("vault_bulk_dismount_partial:syncthing_profile_unavailable:2:1", "dismount");
  expect(message).toContain("2 encrypted volume(s) dismounted; 1 not confirmed dismounted");
  expect(message).toContain("synced folders are safely paused");
  expect(message).not.toContain("Keep the personal Vault mounted");
});
test("untracked driver cleanup explains the recovery action without exposing raw details", () => {
  const message = vaultOperationError("vault_untracked_mounts C:\\private\\container", "dismount");
  expect(message).toContain("restart Windows");
  expect(message).toContain("every signed-in account");
  expect(message).not.toContain("C:\\private");
});
