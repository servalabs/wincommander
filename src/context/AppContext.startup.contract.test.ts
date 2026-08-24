import { describe, expect, test } from "bun:test";

declare const Bun: { file(path: string): { text(): Promise<string> } };

describe("AppContext startup coordination", () => {
  test("recovers a timed-out settings cache and keeps eligibility out of startup effect dependencies", async () => {
    const source = await Bun.file("src/context/AppContext.tsx").text();

    expect(source).toMatch(/id:\s*['"]settings-cache['"]/);
    expect(source).toMatch(/cached\.outcome !== ['"]completed['"] \|\| !cached\.value/);
    expect(source).toContain("hydratedSettings = await initSettings(false)");
    expect(source).toContain("reportStartupPhase('settings_cache_hydrated')");
    expect(source).toMatch(/id:\s*['"]system-probe['"]/);
    expect(source).toMatch(/id:\s*['"]startup-status['"]/);
    expect(source).toContain("startupEligibilityRef.current");
    expect(source).not.toContain("runStartupJob,\n    startupEligibility,");
  });
});
