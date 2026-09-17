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
});
