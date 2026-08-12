import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("StorageOverviewCard accessibility", () => {
  test("reports its collapsed and expanded state", async () => {
    const source = await Bun.file("src/components/dashboard/StorageOverviewCard.tsx").text();

    expect(source).toContain("aria-expanded={expanded}");
  });
});
