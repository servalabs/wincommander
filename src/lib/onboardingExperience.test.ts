import { describe, expect, test } from "bun:test";
import { createBrowserInventoryCache, createOnboardingExperiencePreloader } from "./onboardingExperience";

describe("onboarding experience preload", () => {
  test("shares in-flight browser inventory and reuses a successful result", async () => {
    let calls = 0;
    let resolveLoad: ((value: { success: boolean; value: string }) => void) | undefined;
    const cache = createBrowserInventoryCache(
      () => {
        calls += 1;
        return new Promise<{ success: boolean; value: string }>((resolve) => { resolveLoad = resolve; });
      },
      (result) => result.success,
    );

    const first = cache.get();
    const second = cache.get();
    await Promise.resolve();
    expect(calls).toBe(1);
    resolveLoad?.({ success: true, value: "browser inventory" });
    expect(await first).toEqual({ success: true, value: "browser inventory" });
    expect(await second).toEqual({ success: true, value: "browser inventory" });
    expect(await cache.get()).toEqual({ success: true, value: "browser inventory" });
    expect(calls).toBe(1);
  });

  test("retries failed inventory and refreshes only when requested", async () => {
    let calls = 0;
    const cache = createBrowserInventoryCache(
      async () => ({ success: ++calls > 1, value: calls }),
      (result) => result.success,
    );

    expect(await cache.get()).toEqual({ success: false, value: 1 });
    expect(await cache.get()).toEqual({ success: true, value: 2 });
    expect(await cache.get()).toEqual({ success: true, value: 2 });
    expect(await cache.refresh()).toEqual({ success: true, value: 3 });
    expect(calls).toBe(3);
  });

  test("warms browser data and the Scrub UI chunk once, even when startup calls twice", async () => {
    let browserLoads = 0;
    let scrubUiLoads = 0;
    const preload = createOnboardingExperiencePreloader(
      async () => { browserLoads += 1; },
      async () => { scrubUiLoads += 1; },
    );

    const first = preload();
    const second = preload();
    expect(second).toBe(first);
    await Promise.all([first, second]);
    expect(browserLoads).toBe(1);
    expect(scrubUiLoads).toBe(1);
  });
});
