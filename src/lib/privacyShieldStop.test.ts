import { describe, expect, test } from "bun:test";
import { privacyShieldStopError, privacyShieldStopVerificationError } from "./privacyShieldStop";

describe("Privacy Shield Stop results", () => {
  test("only a positive native result counts as success", () => {
    expect(privacyShieldStopError({ success: true, data: { success: true } })).toBeNull();
    expect(privacyShieldStopError({ success: true, data: { success: false } })).toContain("could not be stopped");
    expect(privacyShieldStopError({ success: true })).toContain("could not be stopped");
  });
  test("explains a busy policy transition", () => {
    expect(privacyShieldStopError({ success: false, error: "machine-state operation is busy; retry shortly" })).toContain("updating");
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
});
