import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("startup logo", () => {
  test("preloads the packaged asset before the main window entry and keeps a first-frame fallback", async () => {
    const [entry, asset, splash, styles] = await Promise.all([
      Bun.file("src/main.tsx").text(),
      Bun.file("src/assets/logoUrl.ts").text(),
      Bun.file("src/components/SplashScreen.tsx").text(),
      Bun.file("src/components/SplashScreen.css").text(),
    ]);

    expect(entry).toContain("preloadAppLogo();");
    expect(asset).toContain('link.rel = "preload";');
    expect(asset).toContain('link.as = "image";');
    expect(splash).toContain('const [logoReady] = useState(true);');
    expect(splash).toContain('fetchPriority="high"');
    expect(splash).toContain('className="sp-logo-fallback"');
    expect(styles).not.toContain("animation: sp-fade-in 0.5s ease-out both;");
  });
});
