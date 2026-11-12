import { describe, expect, test } from "bun:test";
import { recoveryLabEnabled, validateRecoveryLabCargoArgs } from "./recovery-lab-build";

describe("local recovery build selection", () => {
  test("defaults off and requires both explicit switches", () => {
    expect(recoveryLabEnabled({})).toBe(false);
    expect(recoveryLabEnabled({ WINCMD_FLEET_RECOVERY_LAB: "1", VITE_FLEET_RECOVERY_LAB: "1" })).toBe(true);
    expect(() => recoveryLabEnabled({ WINCMD_FLEET_RECOVERY_LAB: "1" })).toThrow();
    expect(() => recoveryLabEnabled({ VITE_FLEET_RECOVERY_LAB: "1" })).toThrow();
  });
  test("forwarded Cargo flags cannot bypass paired switches", () => {
    for (const args of [["--all-features"], ["--features", "fleet-recovery-lab"], ["-F", "other,commander-pro/fleet-recovery-lab"], ["--features=fleet-recovery-lab"], ["-Ffleet-recovery-lab"], ["-F=fleet-recovery-lab"]]) {
      expect(() => validateRecoveryLabCargoArgs(args, false)).toThrow();
      expect(() => validateRecoveryLabCargoArgs(args, true)).not.toThrow();
    }
    expect(() => validateRecoveryLabCargoArgs(["--features", "ordinary"], false)).not.toThrow();
  });
});
