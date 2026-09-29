import { expect, test } from "bun:test";
import { isAuthorizedBulkDismountReceipt, selectableDriveLetters, vaultOperationError } from "./vaultOperationFeedback";

test("bulk success requires an explicit bounded authorized-subset receipt", () => {
  const receipt = { status: "authorized_dismounted", state: "unmounted", scope: "authorized", dismounted: 2 };
  expect(isAuthorizedBulkDismountReceipt(receipt)).toBe(true);
  expect(isAuthorizedBulkDismountReceipt({ ...receipt, dismounted: 0 })).toBe(true);
  for (const value of [null, {}, { status: "ok" }, { ...receipt, scope: "all" }, { ...receipt, state: "mounted" },
    { ...receipt, dismounted: -1 }, { ...receipt, dismounted: 27 }, { ...receipt, dismounted: 1.5 }, { ...receipt, dismounted: "2" }]) {
    expect(isAuthorizedBulkDismountReceipt(value)).toBe(false);
  }
});

test("shows the same unlock guidance for native string and Error rejections", () => {
  const message = vaultOperationError("vault_engine_unlock_failed");
  expect(message).toContain("password, PIM and keyfiles");
  expect(vaultOperationError(new Error("vault_engine_unlock_failed"))).toBe(message);
});

test("missing Pro installation and licensing failure never diagnose the container password", () => {
  for (const error of ["PRO_NOT_INSTALLED: missing engine", "vault_pro_not_installed"]) {
    expect(vaultOperationError(error)).toContain("Pro module is not installed");
    expect(vaultOperationError(error)).not.toContain("password");
  }
  expect(vaultOperationError("vault_entitlement_denied")).toContain("verify or activate your key");
  expect(vaultOperationError("vault_entitlement_denied")).not.toContain("password");
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
