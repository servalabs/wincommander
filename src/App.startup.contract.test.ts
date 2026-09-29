import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("startup prefetch ownership", () => {
  test("personal recovery or unavailable settings replace only Automation before its panel can mount", async () => {
    const source = await Bun.file("src/App.tsx").text();
    const route = source.slice(source.indexOf("function PanelRoute("), source.indexOf("function AppContent("));
    const guard = 'if (panelId === "flows" && personalSettingsStatus && (personalSettingsStatus.recoveryRequired || !personalSettingsStatus.canSave))';

    expect(route).toContain(guard);
    expect(route).toContain("return <FlowSettingsRecovery recoveryRequired={personalSettingsStatus.recoveryRequired} />;");
    expect(route.indexOf(guard)).toBeLessThan(route.indexOf("getLazyPanel("));
    expect(route).not.toContain('panelId === "vault"');
    expect(route).not.toContain('panelId === "productivity"');
    expect(route).toContain('if (manifest?.id === "dashboard") return <DashboardPanel />;');
  });

  test("keeps the splash visible until the readiness gate completes on every launch", async () => {
    const source = await Bun.file("src/App.tsx").text();

    expect(source).toContain("const isLoading = !splashDone;");
    expect(source).not.toContain("const isFirstRunLoading");
  });

  test("waits for browser idle before entering the disk cleanup expensive lane", async () => {
    const source = await Bun.file("src/App.tsx").text();

    expect(source).toContain('const cancelDiskIdle = scheduleWhenIdle(() => preloadDiskCleanup("idle"));');
    expect(source).not.toContain('await queueWhenIdle(signal, []);');
  });

  test("routes idle and hover panel work through the AppContext coordinator", async () => {
    const source = await Bun.file("src/App.tsx").text();

    expect(source).toContain('id: "panel-preload"');
    expect(source).toContain('id: "search-preload"');
    expect(source).toContain('priority: "background"');
    expect(source).toContain("runStartupJob");
  });
});
