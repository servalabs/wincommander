import { describe, expect, it } from "bun:test";
import { bluetoothControlDescription, bluetoothControlIssue, bluetoothControlLabel, canChangeBluetooth, describeBluetoothFailure, parseBluetoothControlStatus, type BluetoothControlStatus } from "./bluetoothControl";

const allowed: BluetoothControlStatus = {
  schemaVersion: 2, blocked: false, state: "allowed", canChange: true, message: "",
};

describe("Bluetooth machine policy presentation", () => {
  it("uses familiar on/off wording while keeping the policy limit in its description", () => {
    expect(bluetoothControlLabel(null)).toBe("Bluetooth unavailable");
    expect(bluetoothControlLabel(allowed)).toBe("Bluetooth ON");
    expect(bluetoothControlDescription(allowed)).toContain("Windows radio settings and other restrictions still apply");
    expect(bluetoothControlLabel({ ...allowed, blocked: true, state: "blocked" })).toBe("Bluetooth OFF");
    expect(bluetoothControlDescription({ ...allowed, blocked: true, state: "blocked" })).toContain("Windows confirms");
  });

  it("never reports pending policy intent as a confirmed block or release", () => {
    for (const blocked of [true, false]) {
      const pending = { ...allowed, state: "pending" as const, blocked };
      expect(parseBluetoothControlStatus(pending)).toEqual(pending);
      expect(bluetoothControlLabel(pending)).toBe("Bluetooth updating…");
      expect(bluetoothControlDescription(pending)).toContain("has not confirmed");
    }
  });

  it("locks unsupported, externally managed, and legacy states even for administrators", () => {
    for (const state of ["unsupported", "externally_managed", "legacy_recovery_required"] as const) {
      const status = { ...allowed, state, canChange: false };
      expect(parseBluetoothControlStatus(status)).toEqual(status);
      expect(canChangeBluetooth(status, true)).toBe(false);
      expect(canChangeBluetooth({ ...status, canChange: true }, true)).toBe(false);
    }
    expect(bluetoothControlLabel({ ...allowed, state: "unsupported", canChange: false })).toBe("Bluetooth unavailable");
    expect(bluetoothControlDescription({ ...allowed, state: "externally_managed", canChange: false })).toContain("administrator");
    expect(bluetoothControlDescription({ ...allowed, state: "legacy_recovery_required", canChange: false })).toContain("Windows Device Manager");
  });

  it("fails closed for non-admin, unknown account, unknown status, and denied backend approval", () => {
    expect(canChangeBluetooth(allowed, true)).toBe(true);
    expect(canChangeBluetooth(allowed, false)).toBe(false);
    expect(canChangeBluetooth(allowed, undefined)).toBe(false);
    expect(canChangeBluetooth(null, true)).toBe(false);
    expect(canChangeBluetooth({ ...allowed, canChange: false }, true)).toBe(false);
    expect(canChangeBluetooth({ ...allowed, state: "pending", blocked: true }, true)).toBe(true);
  });

  it("rejects old-service replies, contradictory success, unknown fields, and malformed policy status", () => {
    expect(parseBluetoothControlStatus(allowed)).toEqual(allowed);
    const invalid = [
      null, {}, { ...allowed, schemaVersion: undefined }, { ...allowed, schemaVersion: 1 },
      { ...allowed, adapterCount: 1, ownedAdapterCount: 0, monitoring: true },
      { ...allowed, state: "no_adapter" }, { ...allowed, blocked: "true" },
      { ...allowed, state: "blocked" }, { ...allowed, blocked: true },
      { ...allowed, state: "unsupported" }, { ...allowed, state: "externally_managed" },
      { ...allowed, state: "legacy_recovery_required" }, { ...allowed, message: "a".repeat(1025) },
    ];
    for (const value of invalid) {
      let failure: unknown;
      try { parseBluetoothControlStatus(value); } catch (error) { failure = error; }
      expect(failure instanceof Error && failure.message).toBe("Bluetooth policy status unavailable");
    }
  });

  it("does not display untrusted policy messages or identifying details", () => {
    const status = { ...allowed, message: "PRIVATE PATH OR IDENTITY" };
    expect(bluetoothControlDescription(status)).not.toContain(status.message);
    expect(bluetoothControlLabel(status)).not.toContain(status.message);
  });

  it("reports actionable issue episodes while leaving confirmed policy states quiet", () => {
    expect(bluetoothControlIssue(allowed)).toBeNull();
    expect(bluetoothControlIssue({ ...allowed, blocked: true, state: "blocked" })).toBeNull();
    expect(bluetoothControlIssue(null)?.key).toBe("unavailable");
    expect(bluetoothControlIssue(null)?.message).toContain("retry automatically");
    expect(bluetoothControlIssue({ ...allowed, state: "pending" })?.key).toBe("pending");
    expect(bluetoothControlIssue({ ...allowed, state: "unsupported", canChange: false })?.key).toBe("unsupported");
    expect(bluetoothControlIssue({ ...allowed, state: "legacy_recovery_required", canChange: false })?.message).toContain("administrator review");
  });

  it("maps only current policy failure tokens to bounded safe next actions", () => {
    const cases = [
      ["BT_POLICY_UNSUPPORTED", "unsupported"], ["BT_POLICY_MANAGED", "organisation policy"],
      ["BT_POLICY_PENDING", "has not confirmed"], ["BT_POLICY_REJECTED", "Windows rejected"],
      ["BT_POLICY_UNVERIFIED", "verify the Windows Bluetooth policy"], ["BT_STATE_UNVERIFIED", "protected Bluetooth policy state"],
      ["BT_OPERATION_BUSY", "already running"], ["BT_ACCESS_DENIED", "administrator"],
      ["BT_LEGACY_RECOVERY", "cannot restore that earlier device change"], ["BT_SERVICE_UNAVAILABLE", "matching WinCommander service"],
    ];
    for (const [code, expected] of cases) {
      const message = describeBluetoothFailure(new Error(`PRIVATE ID [${code}] PRIVATE PATH`));
      expect(message).toContain(expected);
      expect(message).not.toContain("PRIVATE");
    }
    expect(describeBluetoothFailure("PRIVATE UNKNOWN [BT_UNKNOWN]")).toContain("No result is assumed");
    expect(describeBluetoothFailure("PRIVATE [BT_DISABLE_REJECTED]")).toContain("No result is assumed");
    expect(describeBluetoothFailure({ message: "PRIVATE" })).not.toContain("PRIVATE");
    expect(describeBluetoothFailure("PRIVATE", "Safe observation fallback")).toBe("Safe observation fallback");
  });
});
