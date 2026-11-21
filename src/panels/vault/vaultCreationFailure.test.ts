import { describe, expect, test } from "bun:test";
import { vaultCreationFailureDetail } from "./vaultCreationFailure";

describe("Vault creation failure presentation", () => {
  test("pending driver activation requires Windows restart without exposing service details", () => {
    const detail = vaultCreationFailureDetail(new Error("vault_driver_unavailable (encryption_driver_restart_required: C:\\private\\vault.hc secret-value)"));
    expect(detail).toContain("restart Windows");
    expect(detail).toContain("try creating the Vault again");
    expect(detail).toContain("still dismount existing vaults");
    expect(detail).toContain("Restarting WinCommander alone will not activate");
    for (const raw of ["repair", "private", "secret-value"]) expect(detail).not.toContain(raw);
  });

  test("turns driver and helper failures into safe, actionable guidance", () => {
    expect(vaultCreationFailureDetail("vault_driver_unavailable")).toContain("driver could not be prepared");
    expect(vaultCreationFailureDetail("vault_broker_unavailable")).toContain("helper is missing");
  });

  test("tells the user when the chosen Vault name is already taken", () => {
    const detail = vaultCreationFailureDetail(new Error("service rejected request: vault_target_exists (a file or folder with that name already exists)"));
    expect(detail).toContain("already exists");
    expect(detail).toContain("Choose a different name");
    expect(detail).not.toContain("ownership");
    expect(detail).not.toContain("vault_target_exists");
  });

  test("tells the user when the chosen file is governed by Vault permissions", () => {
    const detail = vaultCreationFailureDetail("vault_path_reserved");
    expect(detail).toContain("governed by Vault permissions");
    expect(detail).not.toContain("vault_path_reserved");
  });

  test("an unrecorded owner is its own failure, never confused with a taken name", () => {
    const detail = vaultCreationFailureDetail("vault_owner_record_failed");
    expect(detail).toContain("ownership");
    expect(detail).not.toContain("already exists");
  });

  test("does not surface legacy opaque failure keys", () => {
    expect(vaultCreationFailureDetail("vault_create_failed")).not.toContain("vault_create_failed");
  });

  test("gives a safe, actionable explanation for rejected partition requests", () => {
    const detail = vaultCreationFailureDetail("vault_validation_failed");

    expect(detail).toContain("partition");
    expect(detail).toContain("No Vault was created");
    expect(detail).toContain("VLT.CREATE.VALIDATION_FAILED");
    expect(detail).not.toContain("vault_validation_failed");
  });
});
