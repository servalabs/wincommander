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

  test("explains a space shortage with the 1x and 2x figures and no raw code", () => {
    const detail = vaultCreationFailureDetail(new Error("vault_creation_preparation_failed (C:\\private\\vault.hc)"));
    expect(detail).toContain("not enough free space");
    expect(detail).toContain("twice the Vault size");
    expect(detail).toContain("not running as an administrator");
    expect(detail).not.toContain("vault_creation_preparation_failed");
    expect(detail).not.toContain("private");
  });

  test("a Vault larger than the destination is its own message", () => {
    const detail = vaultCreationFailureDetail("vault_container_too_large");
    expect(detail).toContain("larger than the free space");
    expect(detail).toContain("smaller size");
    expect(detail).not.toContain("vault_container_too_large");
  });

  test("a hidden volume that does not fit says to reduce its size", () => {
    const detail = vaultCreationFailureDetail("vault_hidden_too_large");
    expect(detail).toContain("reduce the hidden volume size");
    expect(detail).not.toContain("vault_hidden_too_large");
  });

  test("a volume held open names antivirus and indexing and asks for a retry", () => {
    const detail = vaultCreationFailureDetail("vault_format_files_open");
    expect(detail).toContain("antivirus");
    expect(detail).toContain("indexing");
    expect(detail).toContain("Try again");
    expect(detail).not.toContain("vault_format_files_open");
  });

  test("a creation timeout tells the owner large Vaults take long and to retry", () => {
    const detail = vaultCreationFailureDetail("vault_create_timeout");
    expect(detail).toContain("Large Vaults");
    expect(detail).toContain("Try again");
    expect(detail).not.toContain("vault_create_timeout");
  });

  test("an unknown code still falls to the generic message without leaking it", () => {
    const detail = vaultCreationFailureDetail("vault_some_future_code");
    expect(detail).toContain("could not create this Vault");
    expect(detail).not.toContain("vault_some_future_code");
  });

  test("gives a safe, actionable explanation for rejected partition requests", () => {
    const detail = vaultCreationFailureDetail("vault_validation_failed");

    expect(detail).toContain("partition");
    expect(detail).toContain("No Vault was created");
    expect(detail).toContain("VLT.CREATE.VALIDATION_FAILED");
    expect(detail).not.toContain("vault_validation_failed");
  });
});
