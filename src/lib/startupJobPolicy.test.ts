import { describe, expect, test } from "bun:test";
import { canRunStartupJob, type StartupEligibility } from "./startupJobPolicy";

const denied: StartupEligibility = {
  hasVerifiedPaidEntitlement: false,
  isProInstalled: false,
  isProConfigured: false,
  autoUpdateEnabled: false,
  meshEnabled: false,
  dependenciesEnabled: false,
  hasIdleWindow: false,
};

describe("startup job policy", () => {
  test("keeps the cached shell path available without optional supervisors", () => {
    expect(canRunStartupJob("settings-cache", denied)).toBe(true);
    expect(canRunStartupJob("startup-status", denied)).toBe(true);
    expect(canRunStartupJob("pro-manifest", denied)).toBe(false);
    expect(canRunStartupJob("defender-status", denied)).toBe(false);
    expect(canRunStartupJob("mesh-status", denied)).toBe(false);
    expect(canRunStartupJob("app-inventory", denied)).toBe(false);
  });

  test("requires paid entitlement, configuration, install state and idle time for optional work", () => {
    const enabled: StartupEligibility = {
      hasVerifiedPaidEntitlement: true,
      isProInstalled: true,
      isProConfigured: true,
      autoUpdateEnabled: true,
      meshEnabled: true,
      dependenciesEnabled: true,
      hasIdleWindow: true,
    };

    expect(canRunStartupJob("pro-manifest", enabled)).toBe(true);
    expect(canRunStartupJob("pro-hash", enabled)).toBe(true);
    expect(canRunStartupJob("defender-status", enabled)).toBe(true);
    expect(canRunStartupJob("dependencies", enabled)).toBe(true);
    expect(canRunStartupJob("mesh-status", enabled)).toBe(true);
    expect(canRunStartupJob("app-inventory", enabled)).toBe(true);
  });
});
