import { describe, expect, test } from "bun:test";
import { normalizeOperationId } from "./notificationStore";

describe("notification operation IDs", () => {
  test("keeps a bounded opaque diagnostic reference", () => {
    expect(normalizeOperationId(" MNT-7QK4:engine_start ")).toBe("MNT-7QK4:engine_start");
  });

  test("drops unsafe or oversized values from the bell", () => {
    expect(normalizeOperationId("vault name: Alice's private vault")).toBeUndefined();
    expect(normalizeOperationId("X".repeat(129))).toBeUndefined();
  });
});
