import { expect, test } from "bun:test";
import { confirmedBulkDismountMessage, confirmedMountObservationError, isAuthorizedBulkDismountReceipt, selectableDriveLetters, vaultOperationError } from "./vaultOperationFeedback";
import type { EncryptionStatus } from "@/hooks/useBackend";

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

test("runtime incompatibility distinguishes preflight denial from unverified post-operation status", () => {
  const preflight = vaultOperationError("vault_runtime_update_required");
  expect(preflight).toContain("matching Pro update from License / Pro");
  expect(preflight).toContain("reinstalling the older Pro will not fix it");
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
  expect(vaultOperationError("vault_caller_access_denied")).toContain("Windows permissions");
  expect(vaultOperationError("unknown", "dismount")).toContain("Close files");
});

test("mounted-root access denial explains where to review permissions without changing container ownership", () => {
  const message = vaultOperationError("vault_caller_access_denied");
  expect(message).toContain("files inside the Vault");
  expect(message).toContain("Its saved permissions were not changed");
  expect(message).toContain("original PC");
  expect(message).toContain("account that can mount it");
  expect(message).toContain("same username on another PC can mean a different Windows account");
  expect(message).toContain("mounted drive in File Explorer");
  expect(message).toContain("Properties > Security > Advanced");
  expect(message).toContain("container file's permissions alone does not change permissions inside it");
  expect(message).toContain("Do not reformat");
  expect(message).not.toContain("take ownership");
  expect(message).not.toContain("password");
  expect(vaultOperationError(new Error("vault_caller_access_denied"))).toBe(message);
});

test("registered ownership and Fleet denial give policy recovery rather than filesystem repair", () => {
  const owner = vaultOperationError("vault_private_owner_required", "dismount");
  expect(owner).toContain("original mounting session");
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
