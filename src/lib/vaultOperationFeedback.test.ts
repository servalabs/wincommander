import { expect, test } from "bun:test";
import { selectableDriveLetters, vaultOperationError } from "./vaultOperationFeedback";

test("shows the same unlock guidance for native string and Error rejections", () => {
  const message = vaultOperationError("vault_engine_unlock_failed");
  expect(message).toContain("password, PIM and keyfiles");
  expect(vaultOperationError(new Error("vault_engine_unlock_failed"))).toBe(message);
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

test("already-mounted requests direct users to the existing drive without diagnosing credentials", () => {
  expect(vaultOperationError("vault_already_mounted")).toContain("Open the existing drive");
  expect(vaultOperationError("vault_already_mounted")).not.toContain("password");
});

test("availability is never invented and other draft reservations are excluded", () => {
  expect(selectableDriveLetters([])).toEqual([]);
  expect(selectableDriveLetters(["v", "W:", "W", "Z", "??", "C:\\"], ["V", "z:"])).toEqual(["W"]);
});
