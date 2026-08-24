import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("startup prefetch ownership", () => {
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
