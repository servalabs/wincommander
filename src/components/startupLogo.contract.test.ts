import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("startup logo", () => {
  test("embeds the existing product logo for the first splash frame", async () => {
    const [entry, asset, splash, styles] = await Promise.all([
      Bun.file("src/main.tsx").text(),
      Bun.file("src/assets/logoUrl.ts").text(),
      Bun.file("src/components/StartupAnimation.tsx").text(),
      Bun.file("src/components/SplashScreen.css").text(),
    ]);

    expect(entry).not.toContain("preloadAppLogo");
    expect(asset).toContain('logo.png?inline');
    expect(splash).toContain('<img src={LOGO_URL}');
    expect(splash).not.toContain('sp-logo-fallback');
    expect(styles).not.toContain('sp-logo-fallback');
    expect(styles).not.toContain("animation: sp-fade-in 0.5s ease-out both;");
  });

  test("keeps the startup surface a restrained blueprint rather than matrix rain", async () => {
    const [splash, styles] = await Promise.all([
      Bun.file("src/components/StartupAnimation.tsx").text(),
      Bun.file("src/components/SplashScreen.css").text(),
    ]);

    expect(splash).toContain('<div className="sp-blueprint-grid"');
    expect(splash).not.toContain("sp-matrix-canvas");
    expect(splash).not.toContain("SCRAMBLE_GLYPHS");
    expect(styles).toContain(".sp-blueprint-grid");
    expect(styles).toContain("--splash-bg: #f4f9fc");
    expect(styles).not.toContain(".sp-matrix-canvas");
  });
});
