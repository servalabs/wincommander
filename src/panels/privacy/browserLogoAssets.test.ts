import { describe, expect, test } from "bun:test";

// Minimal local declaration so this test type-checks under the project's
// tsconfig (which intentionally ships no @types/node / @types/bun).
declare const Bun: {
  file(path: string | URL): { text(): Promise<string> };
};

// The bundled browser logos must be genuine multicolor brand marks, not the
// monochrome single-path glyphs that previously rendered as black blobs.
const COLORFUL_BROWSER_LOGOS = [
  "chrome.svg",
  "edge.svg",
  "firefox.svg",
  "opera.svg",
  "opera-gx.svg",
  "brave.svg",
  "vivaldi.svg",
  "librewolf.svg",
  "floorp.svg",
] as const;

function collectPaintTokens(svg: string): string[] {
  const attrMatches = [...svg.matchAll(/(?:fill|stroke|stop-color)\s*=\s*"([^"]+)"/gi)].map(
    (match) => match[1].trim().toLowerCase(),
  );
  const styleMatches = [...svg.matchAll(/(?:fill|stroke|stop-color)\s*:\s*([^;"'}]+)/gi)].map(
    (match) => match[1].trim().toLowerCase(),
  );
  return [...new Set([...attrMatches, ...styleMatches])].filter(
    (token) =>
      token !== "none" &&
      token !== "currentcolor" &&
      token !== "#000" &&
      token !== "#000000" &&
      token !== "black",
  );
}

function hasColorPaint(svg: string): boolean {
  const paints = collectPaintTokens(svg);
  return paints.length >= 1 || /linearGradient|radialGradient/i.test(svg);
}

describe("bundled browser logo assets", () => {
  for (const fileName of COLORFUL_BROWSER_LOGOS) {
    test(`${fileName} is a multicolor brand mark`, async () => {
      const svg = await Bun.file(new URL(`../../assets/browser-logos/${fileName}`, import.meta.url)).text();
      expect(hasColorPaint(svg)).toBe(true);
    });
  }
});
