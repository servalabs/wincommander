import { describe, expect, test } from "bun:test";
import type { AppSettings } from "../types/settings";
import { getSettingsWriteFailure, recoverSettingsWrite, runSharedSettingsRecoveryRead } from "./settingsWriteRecovery";

describe("settings write recovery", () => {
  test("distinguishes a blocked settings write from an ordinary failure", () => {
    expect(getSettingsWriteFailure(new Error("Access denied: requires elevation"))).toEqual({
      state: "Blocked",
      reason: "Access denied: requires elevation",
    });
    expect(getSettingsWriteFailure(new Error("store is read-only"))).toEqual({
      state: "Blocked",
      reason: "store is read-only",
    });
    expect(getSettingsWriteFailure(new Error("Requires elevation"))).toEqual({
      state: "Blocked",
      reason: "Requires elevation",
    });
    expect(getSettingsWriteFailure(new Error("Personal settings are temporary; restore or update the WinCommander service before saving"))).toEqual({
      state: "Blocked",
      reason: "Personal settings are temporary; restore or update the WinCommander service before saving",
    });
    expect(getSettingsWriteFailure(new Error("Personal settings service is still unavailable; no preferences were saved"))).toEqual({
      state: "Blocked",
      reason: "Personal settings service is still unavailable; no preferences were saved",
    });
    expect(getSettingsWriteFailure(new Error("Personal settings service recovered. Refresh settings before saving so newer preferences are preserved."))).toEqual({
      state: "Blocked",
      reason: "Personal settings service recovered. Refresh settings before saving so newer preferences are preserved.",
    });
    expect(getSettingsWriteFailure(new Error("IPC disconnected"))).toEqual({
      state: "Failed",
      reason: "IPC disconnected",
    });
  });

  test("restores the authoritative settings snapshot after a rejected write", async () => {
    const restored = { app: { theme: "dark" } } as AppSettings;
    let applied: AppSettings | null = null;

    let failure: unknown = null;
    await recoverSettingsWrite(
      async () => { throw new Error("store is read-only"); },
      async () => restored,
      (settings) => { applied = settings; },
      () => {},
    ).catch((error) => { failure = error; });

    expect(applied).toBe(restored);
    expect((failure as Error).message).toBe("store is read-only");
  });

  test("shares a pending authoritative recovery read between automatic and manual checks", async () => {
    const slot = { pending: null as Promise<string> | null };
    let resolve!: (value: string) => void;
    let calls = 0;
    const pending = new Promise<string>((done) => { resolve = done; });
    const read = () => { calls += 1; return pending; };

    const automatic = runSharedSettingsRecoveryRead(slot, read);
    const manual = runSharedSettingsRecoveryRead(slot, read);
    expect(manual).toBe(automatic);
    await Promise.resolve();
    expect(calls).toBe(1);

    resolve("service record");
    await expect(automatic).resolves.toBe("service record");
    expect(slot.pending).toBeNull();
  });
});
