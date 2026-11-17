import { describe, expect, test } from "bun:test";
import { vaultCreationFailureDetail } from "./vaultCreationFailure";

describe("Vault creation failure presentation", () => {
  test("turns driver and helper failures into safe, actionable guidance", () => {
    expect(vaultCreationFailureDetail("vault_driver_unavailable")).toContain("driver could not be prepared");
    expect(vaultCreationFailureDetail("vault_broker_unavailable")).toContain("helper is missing");
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
