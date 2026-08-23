import { describe, expect, test } from "bun:test";
import { createStartupProtectionReadiness } from "./startupProtectionReadiness";

describe("startup protection readiness", () => {
  test("reports not required only when no startup-critical protection is configured", () => {
    const events: string[] = [];
    expect(createStartupProtectionReadiness((event) => events.push(event)).configure([])).toBe("protection_not_required");
    expect(events).toEqual(["protection_not_required"]);
  });

  test("waits for every actual protection result and fails closed on a real failure", () => {
    const events: string[] = [];
    const readiness = createStartupProtectionReadiness((event) => events.push(event));
    readiness.configure(["decoy-monitor", "ransomware-monitor"]);
    expect(readiness.report("decoy-monitor", true)).toBeNull();
    expect(readiness.report("ransomware-monitor", false)).toBe("protection_failed");
    expect(readiness.report("ransomware-monitor", true)).toBeNull();
    expect(events).toEqual(["protection_failed"]);
  });

  test("locks the configured startup set so a later settings change cannot redefine launch readiness", () => {
    const events: string[] = [];
    const readiness = createStartupProtectionReadiness((event) => events.push(event));
    readiness.configure(["ransomware-monitor"]);
    readiness.configure([]);
    expect(readiness.report("ransomware-monitor", true)).toBe("protection_required_ready");
    expect(events).toEqual(["protection_required_ready"]);
  });
});
