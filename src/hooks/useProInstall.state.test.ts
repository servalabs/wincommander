import { describe, expect, test } from "bun:test";
import { proInstallStateForNewAttempt, type InstallState } from "./useProInstall";

describe("Pro install attempt state", () => {
  test("does not reuse a previous success when a later release check fails", () => {
    const previous: InstallState = { kind: "installed", version: "3.6.4" };
    const next = proInstallStateForNewAttempt(previous);
    expect(next).toEqual({ kind: "idle" });
    expect(next.kind === "installed").toBe(false);
  });

  test("clears a terminal error for a deliberate new attempt", () => {
    expect(proInstallStateForNewAttempt({ kind: "error", stage: "validation", message: "Unavailable release" }))
      .toEqual({ kind: "idle" });
  });

  test("never resets or duplicates an installation already in flight", () => {
    const active: InstallState = { kind: "installing" };
    expect(proInstallStateForNewAttempt(active)).toBe(active);
    const idle: InstallState = { kind: "idle" };
    expect(proInstallStateForNewAttempt(idle)).toBe(idle);
  });
});
