import { describe, expect, test } from "bun:test";
import { inactivityPresentation } from "./inactivityPresentation";

describe("local inactivity status", () => {
  test("never describes missing, malformed, stopped or disabled state as running", () => {
    for (const state of [null, undefined, {}, "new_version", "watchdog_stopped", "settings_unavailable", "invalid_baseline", "lockdown_disabled", "check_failed", "starting", "checking", "check_stale"]) {
      expect(inactivityPresentation(state).label).not.toBe("Running");
    }
    expect(inactivityPresentation("monitoring").label).toBe("Running");
    expect(inactivityPresentation("monitoring").message).toContain("historical");
    expect(inactivityPresentation("monitoring").message).toContain("may have skipped");
  });
  test("distinguishes recorded trigger from a completed action", () => {
    expect(inactivityPresentation("triggered").message).toContain("does not confirm");
    expect(inactivityPresentation("lockdown_disabled").message).toContain("still record a trigger");
  });
});
