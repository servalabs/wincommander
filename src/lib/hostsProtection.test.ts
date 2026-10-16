import { describe, expect, test } from "bun:test";
import { hostsVerificationLabel, requireHostsChangeReceipt } from "./hostsProtection";

describe("hosts protection results", () => {
  test("only an explicit Windows verification receipt counts as success", () => {
    for (const result of [{ success: true }, { success: true, data: { status: "success", verified: false } }, { success: false, error: "Access denied" }]) {
      let error = "";
      try { requireHostsChangeReceipt(result); } catch (cause) { error = (cause as Error).message; }
      expect(error.length > 0).toBe(true);
      if (result.error) expect(error).toBe(result.error);
    }
    expect(requireHostsChangeReceipt({ success: true, data: { status: "success", verified: true, dnsWarning: "Cache unavailable" } }).dnsWarning).toBe("Cache unavailable");
  });

  test("missing verification never appears as applied", () => {
    expect(hostsVerificationLabel()).toBe("Verification unavailable");
    expect(hostsVerificationLabel({ state: "partial", message: "", matched: 2, expected: 3 })).toBe("Partially applied");
    expect(hostsVerificationLabel({ state: "attention", message: "", matched: 3, expected: 3 })).toBe("Needs attention");
  });
});
