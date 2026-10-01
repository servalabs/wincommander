import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("metadata scrubber report wording", () => {
  test("distinguishes preview findings from verified removal", async () => {
    const source = await Bun.file("src/components/MetadataScrubberDialog.tsx").text();

    expect(source).toContain("Detected in original — will remove");
    expect(source).toContain("Verified removed");
    expect(source).toContain("No removable metadata found.");
    expect(source).toContain("page count and PDF version, is not personal information.");
  });

  test("gives every completed file a private removal receipt and output location", async () => {
    const source = await Bun.file("src/components/MetadataScrubberDialog.tsx").text();

    expect(source).toContain("details?: string[]");
    expect(source).toContain("Removed from this file:");
    expect(source).toContain("This receipt lists property names; the clean copy contains none of their original values.");
    expect(source).toContain("Location removed:");
    expect(source).toContain("Clean copy saved at:");
  });

  test("keeps a large report on one continuous, responsive scroll surface", async () => {
    const source = await Bun.file("src/components/MetadataScrubberDialog.tsx").text();
    const fileCardList = source.slice(
      source.indexOf("function FileCardList"),
      source.indexOf("// ─────────────────────────────────────────────────────────────────────\n// GpsCoordList"),
    );

    expect(source).toContain("height: report ? 'min(88vh, 920px)' : undefined");
    expect(source).toContain('className="wc-dialog-body custom-scrollbar"');
    expect(source).toContain("overscrollBehavior: 'contain'");
    expect(fileCardList).toContain("repeat(auto-fit, minmax(min(300px, 100%), 1fr))");
    expect(fileCardList).not.toContain("overflowY:");
    expect(fileCardList).not.toContain("maxHeight: results.length");
  });
});
