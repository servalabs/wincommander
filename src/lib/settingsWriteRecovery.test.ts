import { describe, expect, test } from "bun:test";
import type { AppSettings } from "../types/settings";
import { getSettingsWriteFailure, recoverSettingsWrite } from "./settingsWriteRecovery";

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
});
