import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("startup logo", () => {
  test("embeds the existing product logo for the first splash frame", async () => {
    const [entry, asset, splash, styles] = await Promise.all([
      Bun.file("src/main.tsx").text(),
      Bun.file("src/assets/logoUrl.ts").text(),
      Bun.file("src/components/SplashScreen.tsx").text(),
      Bun.file("src/components/SplashScreen.css").text(),
    ]);

    expect(entry).not.toContain("preloadAppLogo");
    expect(asset).toContain('logo.png?inline');
    expect(splash).toContain('const [logoReady] = useState(true);');
    expect(splash).not.toContain('sp-logo-fallback');
    expect(styles).not.toContain('sp-logo-fallback');
    expect(styles).not.toContain("animation: sp-fade-in 0.5s ease-out both;");
  });
});
