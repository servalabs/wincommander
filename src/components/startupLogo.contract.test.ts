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
    expect(splash).toContain('const [scrambleText, setScrambleText] = useState');
    expect(splash).not.toContain('sp-logo-fallback');
    expect(styles).not.toContain('sp-logo-fallback');
    expect(styles).not.toContain("animation: sp-fade-in 0.5s ease-out both;");
  });

  test("keeps the release Matrix animation rather than replacing it", async () => {
    const [splash, styles] = await Promise.all([
      Bun.file("src/components/StartupAnimation.tsx").text(),
      Bun.file("src/components/SplashScreen.css").text(),
    ]);

    expect(splash).toContain('className="sp-matrix-canvas"');
    expect(splash).toContain("SCRAMBLE_GLYPHS");
    expect(styles).toContain(".sp-matrix-canvas");
    expect(styles).toContain("--color-bg-primary: #0a0f12");
    expect(styles).not.toContain(".sp-blueprint-grid");
  });
});
