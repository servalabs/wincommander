import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("metadata GPS map viewport", () => {
  test("uses an origin-only referrer so the tile provider can render the preview", async () => {
    const [page, source, dialog] = await Promise.all([
      Bun.file("public/leaflet/gps-map.html").text(),
      Bun.file("public/leaflet/map-init.js").text(),
      Bun.file("src/components/MetadataScrubberDialog.tsx").text(),
    ]);

    expect(page).toContain('<meta name="referrer" content="origin">');
    expect(source).toContain('referrerPolicy: "origin"');
    expect(dialog).toContain('sandbox="allow-scripts allow-same-origin"');
    expect(dialog).toContain('referrerPolicy="origin"');
    expect(dialog).not.toContain('referrerPolicy="no-referrer"');
  });

  test("does not overwrite a user-selected viewport when marker coordinates are unchanged", async () => {
    const source = await Bun.file("public/leaflet/map-init.js").text();

    expect(source).toContain("var viewportKey = null;");
    expect(source).toContain("if (nextViewportKey === viewportKey) return;");
    expect(source).toContain("viewportKey = nextViewportKey;");
  });
});
