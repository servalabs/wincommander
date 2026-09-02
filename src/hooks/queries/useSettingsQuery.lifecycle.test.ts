import { describe, expect, test } from "bun:test";
import { getControlLifecycle } from "./useSettingsQuery";

describe("machine-control lifecycle", () => {
  test("does not call a saved intent applied without matching Windows read-back", () => {
    expect(getControlLifecycle({ desired: true, observed: null })).toEqual({
      state: "Desired",
      reason: null,
      account: null,
    });
    expect(getControlLifecycle({ desired: true, observed: false })).toEqual({
      state: "Desired",
      reason: null,
      account: null,
    });
    expect(getControlLifecycle({ desired: true, observed: true })).toEqual({
      state: "Applied",
      reason: null,
      account: null,
    });
  });

  test("reports an explicit elevation block before offering a machine write", () => {
    expect(getControlLifecycle({ desired: true, observed: true, needsElevation: true })).toEqual({
      state: "Blocked",
      reason: "needs-elevation",
      account: null,
    });
  });

  test("keeps applying, failed, blocked, and unsupported states distinct", () => {
    expect(getControlLifecycle({ applying: true })).toEqual({ state: "Applying", reason: null, account: null });
    expect(getControlLifecycle({ failureReason: "store-read-only" })).toEqual({
      state: "Blocked",
      reason: "store-read-only",
      account: null,
    });
    expect(getControlLifecycle({ failureReason: "Windows read-back mismatch" })).toEqual({
      state: "Failed",
      reason: "Windows read-back mismatch",
      account: null,
    });
    expect(getControlLifecycle({ supported: false })).toEqual({ state: "Not supported", reason: null, account: null });
  });

  test("retains the live Windows account with the visible state and reason", () => {
    expect(getControlLifecycle({
      failureReason: "store-read-only",
      account: { name: "parth", displayName: "Parth", sid: "S-1-5-21" },
    })).toEqual({
      state: "Blocked",
      reason: "store-read-only",
      account: { name: "parth", displayName: "Parth", sid: "S-1-5-21" },
    });
  });
});
