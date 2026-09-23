import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("Privacy Monitor preflight", () => {
  test("starts browser and camera checks together when Monitor opens", async () => {
    const source = await Bun.file("src/panels/privacy/index.tsx").text();
    const start = source.indexOf('if (!showMonitoring || activeTab !== "monitor") return;');
    const end = source.indexOf('}, [activeTab, showMonitoring]);', start);
    const preflight = source.slice(start, end);

    expect(start).toBeGreaterThan(-1);
    expect(preflight).toContain("Promise.allSettled([");
    expect(preflight).toContain("getBrowserInventory()");
    expect(preflight).toContain('executeBackendCommand("Get-PrivacyShieldStatus")');
    expect(preflight).not.toContain("Get-InstalledBrowsersJson");
  });

  test("browser preflight shares the cached inventory request with startup preload", async () => {
    const source = await Bun.file("src/lib/onboardingExperience.ts").text();

    expect(source).toContain("createBrowserInventoryCache");
    expect(source).toContain('Get-InstalledBrowsersJson');
    expect(source).toContain("export const getBrowserInventory = browserInventoryCache.get;");
  });

  test("places Browser Hardening first while Shield status keeps loading in parallel", async () => {
    const source = await Bun.file("src/panels/privacy/index.tsx").text();

    expect(source.indexOf("<BrowserHardeningSection") < source.indexOf("<PrivacyShieldCard />")).toBe(true);
  });
});
