import { describe, expect, it } from "bun:test";
import { bluetoothControlDescription, bluetoothControlIssue, bluetoothControlLabel, canChangeBluetooth, describeBluetoothFailure, parseBluetoothControlStatus, type BluetoothControlStatus } from "./bluetoothControl";

const allowed: BluetoothControlStatus = {
  blocked: false, state: "allowed", adapterCount: 1, ownedAdapterCount: 0,
  canChange: true, monitoring: true, message: "",
};

describe("Bluetooth machine control presentation", () => {
  it("never describes an absent or unverified observation as Bluetooth ON", () => {
    expect(bluetoothControlLabel(null)).toBe("Bluetooth unknown");
    expect(bluetoothControlLabel({ ...allowed, state: "no_adapter", adapterCount: 0 })).toBe("No Bluetooth adapter");
    expect(bluetoothControlLabel(allowed)).toBe("Bluetooth allowed");
    expect(bluetoothControlDescription(allowed)).toContain("another policy may still keep Bluetooth off");
  });

  it("distinguishes persisted block intent from successful adapter blocking", () => {
    expect(bluetoothControlLabel({ ...allowed, blocked: true, state: "partial" })).toBe("Bluetooth partly blocked");
    expect(bluetoothControlLabel({ ...allowed, blocked: true, state: "recovery_required" })).toBe("Bluetooth needs recovery");
    expect(bluetoothControlLabel({ ...allowed, blocked: true, state: "no_adapter", adapterCount: 0 })).toBe("Bluetooth block saved");
    expect(bluetoothControlLabel({ ...allowed, blocked: true, state: "blocked" })).toBe("Bluetooth blocked");
  });

  it("fails closed for non-admin, unknown account, unknown state, or denied backend permission", () => {
    expect(canChangeBluetooth(allowed, true)).toBe(true);
    expect(canChangeBluetooth(allowed, false)).toBe(false);
    expect(canChangeBluetooth(allowed, undefined)).toBe(false);
    expect(canChangeBluetooth(null, true)).toBe(false);
    expect(canChangeBluetooth({ ...allowed, canChange: false }, true)).toBe(false);
  });

  it("allows saving a future block without an attached adapter when the backend approves", () => {
    expect(canChangeBluetooth({ ...allowed, state: "no_adapter", adapterCount: 0 }, true)).toBe(true);
  });

  it("keeps external restrictions and stopped enforcement visible", () => {
    expect(bluetoothControlDescription({ ...allowed, state: "externally_blocked" })).toContain("will not override");
    expect(bluetoothControlDescription({ ...allowed, monitoring: false })).toContain("before a new block");
    expect(bluetoothControlDescription({ ...allowed, blocked: true, monitoring: false })).toContain("release WinCommander");
    expect(bluetoothControlDescription({ ...allowed, blocked: true, state: "blocked" })).toContain("checked in the background");
    expect(bluetoothControlDescription({ ...allowed, state: "recovery_required" })).toContain("Windows Device Manager");
    expect(bluetoothControlDescription({ ...allowed, state: "recovery_required" })).toContain("will not enable an unverified adapter");
  });

  it("rejects incompatible, missing, and malformed status responses", () => {
    expect(parseBluetoothControlStatus(allowed)).toEqual(allowed);
    for (const invalid of [null, {}, { ...allowed, monitoring: undefined }, { ...allowed, adapterCount: -1 }, { ...allowed, adapterCount: 129 }, { ...allowed, ownedAdapterCount: 129 }, { ...allowed, state: "on" }, { ...allowed, blocked: "true" }, { ...allowed, state: "blocked" }, { ...allowed, blocked: true }, { ...allowed, adapterCount: 0 }, { ...allowed, state: "blocked", blocked: true, adapterCount: 0 }, { ...allowed, state: "no_adapter" }, { ...allowed, message: "a".repeat(1025) }]) {
      let failure: unknown;
      try { parseBluetoothControlStatus(invalid); } catch (error) { failure = error; }
      expect(failure instanceof Error && failure.message).toBe("Bluetooth status unavailable");
    }
  });

  it("never renders an untrusted backend message containing device identifiers", () => {
    const status = { ...allowed, message: "PRIVATE DEVICE IDENTIFIER" };
    expect(bluetoothControlDescription(status)).not.toContain(status.message);
    expect(bluetoothControlLabel(status)).not.toContain(status.message);
  });

  it("identifies actionable notification episodes without manual refresh instructions", () => {
    expect(bluetoothControlIssue(allowed)).toBeNull();
    expect(bluetoothControlIssue({ ...allowed, state: "no_adapter", adapterCount: 0 })).toBeNull();
    expect(bluetoothControlIssue(null)?.key).toBe("unavailable");
    expect(bluetoothControlIssue(null)?.message).toContain("retry automatically");
    expect(bluetoothControlIssue({ ...allowed, monitoring: false })?.key).toBe("service:false");
    expect(bluetoothControlIssue({ ...allowed, state: "recovery_required" })?.message).toContain("Windows Device Manager");
    expect(bluetoothControlIssue({ ...allowed, state: "partial", blocked: true })?.message).not.toContain("Refresh");
  });

  it("maps known Windows failure stages to bounded safe next actions", () => {
    const cases = [
      ["BT_ACCESS_DENIED", "administrator"], ["BT_DEVICE_CHANGED", "disconnected"],
      ["BT_RESTART_REQUIRED", "Windows reported that this adapter change requires a restart"], ["BT_DISABLE_REJECTED", "rejected blocking"],
      ["BT_ENABLE_REJECTED", "rejected restoring"], ["BT_TRANSITION_TIMEOUT", "No success is assumed"],
      ["BT_STATE_UNVERIFIED", "protected Bluetooth recovery state"], ["BT_OPERATION_BUSY", "already running"],
      ["BT_SERVICE_UNAVAILABLE", "service connection could not be verified"],
    ];
    for (const [code, expected] of cases) {
      const message = describeBluetoothFailure(new Error(`PRIVATE ID [${code}] PRIVATE PATH`));
      expect(message).toContain(expected);
      expect(message).not.toContain("PRIVATE");
    }
    expect(describeBluetoothFailure("PRIVATE UNKNOWN [BT_UNKNOWN]")).toContain("No result is assumed");
    expect(describeBluetoothFailure({ message: "PRIVATE" })).not.toContain("PRIVATE");
    expect(describeBluetoothFailure("PRIVATE", "Safe status observation fallback")).toBe("Safe status observation fallback");
  });
});
