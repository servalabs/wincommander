import { describe, expect, test } from "bun:test";
import {
  automaticUpdatesAllowedForBuild,
  canAutomaticallyInstallMachineUpdate,
  canAutomaticallyUpdatePro,
} from "./useAutomaticUpdate";

describe("automatic Pro updates", () => {
  test("remain disabled until a release build is positively identified", () => {
    expect(automaticUpdatesAllowedForBuild(null)).toBe(false);
    expect(automaticUpdatesAllowedForBuild(true)).toBe(false);
    expect(automaticUpdatesAllowedForBuild(false)).toBe(true);
  });

  test("never starts a machine-wide installer automatically from a normal token", () => {
    expect(canAutomaticallyInstallMachineUpdate(true, false)).toBe(false);
    expect(canAutomaticallyInstallMachineUpdate(false, true)).toBe(false);
    expect(canAutomaticallyInstallMachineUpdate(true, true)).toBe(true);
  });

    test("updates an installed Pro copy without requiring a new Defender exclusion", () => {
        expect(canAutomaticallyUpdatePro(true, true)).toBe(true);
    });

    test("never auto-installs Pro without update coverage or with a missing sidecar", () => {
        expect(canAutomaticallyUpdatePro(false, true)).toBe(false);
        expect(canAutomaticallyUpdatePro(true, false)).toBe(false);
        expect(canAutomaticallyUpdatePro(true, null)).toBe(false);
    });
});
