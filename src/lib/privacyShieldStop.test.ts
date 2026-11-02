import { describe, expect, test } from "bun:test";
import { confirmPrivacyShieldStop, privacyShieldStopError, privacyShieldStopVerificationError } from "./privacyShieldStop";

describe("Privacy Shield Stop results", () => {
  test("only a positive native result counts as success", () => {
    expect(privacyShieldStopError({ success: true, data: { success: true } })).toBeNull();
    expect(privacyShieldStopError({ success: true, data: { success: false } })).toContain("could not be stopped");
    expect(privacyShieldStopError({ success: true })).toContain("could not be stopped");
  });
  test("explains a busy policy transition", () => {
    expect(privacyShieldStopError({ success: false, error: "machine-state operation is busy; retry shortly" })).toContain("updating");
    expect(privacyShieldStopError({ success: false, error: "Privacy Shield policy is still changing. Wait for the current transition to finish, then retry." })).toContain("updating");
  });
  test("distinguishes an unverifiable detector state from a generic Stop error", () => {
    expect(privacyShieldStopError({ success: false, error: "Privacy Shield process identity changed before stop." })).toContain("Refresh its status");
  });
  test("does not expose arbitrary backend error details", () => {
    expect(privacyShieldStopError({ success: false, error: "private device detail" })).not.toContain("private device detail");
  });
  test("requires a fresh stopped observation after acceptance", () => {
    expect(privacyShieldStopVerificationError({ success: true, data: { running: false } })).toBeNull();
    expect(privacyShieldStopVerificationError({ success: true, data: { running: true } })).toContain("could not be confirmed");
    expect(privacyShieldStopVerificationError({ success: false })).toContain("could not be confirmed");
    expect(privacyShieldStopVerificationError({ success: true, data: {} })).toContain("could not be confirmed");
    expect(privacyShieldStopVerificationError({ success: true, data: { running: false, error: true } })).toContain("could not be confirmed");
    expect(privacyShieldStopError({ success: true, data: { success: true, error: true } })).toContain("could not be stopped");
  });
  test("uses the verified stopped receipt without another hardware probe", async () => {
    let probes = 0;
    expect(await confirmPrivacyShieldStop({ success: true, data: {
      success: true, running: false, shieldLifecycle: "stopped",
    } }, async () => { probes++; return { success: false }; })).toBeNull();
    expect(probes).toBe(0);
  });
  test("success-only legacy results require fresh verification, and failed Stops never report stopped", async () => {
    expect(await confirmPrivacyShieldStop({ success: true, data: { success: true } },
      async () => ({ success: true, data: { running: true } }))).toContain("could not be confirmed");
    let probes = 0;
    expect(await confirmPrivacyShieldStop({ success: false, error: "machine-state operation is busy" },
      async () => { probes++; return { success: true, data: { running: false } }; })).toContain("updating");
    expect(probes).toBe(0);
  });
});
