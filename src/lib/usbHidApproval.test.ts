import { describe, expect, test } from "bun:test";
import {
  canApproveWithPointer,
  isValidVisualChallenge,
  pendingApprovals,
  type PendingUsbHidApproval,
} from "./usbHidApproval";

const pending = (status: PendingUsbHidApproval["status"]): PendingUsbHidApproval => ({
  deviceKey: "hid:example",
  friendlyName: "Example keyboard",
  manufacturer: null,
  vid: "1234",
  pid: "5678",
  serialStable: true,
  topologyVerified: true,
  requestedAt: "2026-08-21T00:00:00Z",
  expiresAt: "2026-08-21T00:01:00Z",
  expiresAtEpoch: 1,
  status,
  availableActions: status === "pending" ? ["allowOnce", "trustAlways", "block"] : [],
  enforcement: { attempted: true, succeeded: true },
  reactive: true,
  firstKeystrokePrevention: false,
  prebootPrevention: false,
});

describe("USB HID approval boundary", () => {
  test("only offers unresolved devices for approval", () => {
    expect(pendingApprovals([
      pending("pending"),
      pending("containmentFailed"),
      pending("blocked"),
      pending("expired"),
    ])).toHaveLength(2);
  });

  test("does not let keyboard activation approve a device", () => {
    expect(canApproveWithPointer("mouse", 0)).toBe(true);
    expect(canApproveWithPointer("mouse", 1)).toBe(false);
    expect(canApproveWithPointer("touch", 0)).toBe(false);
    expect(canApproveWithPointer("keyboard", 0)).toBe(false);
  });

  test("accepts only complete backend-owned challenge progress and keypad state", () => {
    const base = {
      challengeId: "challenge",
      deviceKey: "hid:example",
      action: "allowOnce" as const,
      displaySequence: "481027",
      keypadLayout: ["4", "8", "1", "0", "2", "7", "9", "3", "5", "6"],
      digitsAccepted: 0,
      digitsRemaining: 6,
      step: 1,
      totalSteps: 1,
      expiresAtEpoch: 1,
      expiresAt: "2026-08-21T00:01:00Z",
      reactive: true as const,
      firstKeystrokePrevention: false as const,
      prebootPrevention: false as const,
      userPresenceOnly: true as const,
      sourceAttribution: "unavailable" as const,
      warning: "Visual confirmation is not physical mouse attribution.",
    };
    expect(isValidVisualChallenge(base)).toBe(true);
    expect(isValidVisualChallenge({ ...base, displaySequence: "not-a-code" })).toBe(false);
    expect(isValidVisualChallenge({ ...base, keypadLayout: Array(10).fill("1") })).toBe(false);
    expect(isValidVisualChallenge({ ...base, digitsAccepted: 1, digitsRemaining: 6 })).toBe(false);
  });
});
