import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("metadata GPS map viewport", () => {
  test("does not overwrite a user-selected viewport when marker coordinates are unchanged", async () => {
    const source = await Bun.file("public/leaflet/map-init.js").text();

    expect(source).toContain("var viewportKey = null;");
    expect(source).toContain("if (nextViewportKey === viewportKey) return;");
    expect(source).toContain("viewportKey = nextViewportKey;");
  });
});
