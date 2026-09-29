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
});
