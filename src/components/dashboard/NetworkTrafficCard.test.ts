import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("NetworkTrafficCard alert configuration", () => {
  test("renders one shared upload/download configuration drawer", async () => {
    const source = await Bun.file("src/components/dashboard/NetworkTrafficCard.tsx").text();

    expect((source.match(/<MetricAlertRow metric="upload"/g) ?? []).length).toBe(1);
    expect((source.match(/<MetricAlertRow metric="download"/g) ?? []).length).toBe(1);
    expect(source).toContain("Both alerts are off");
    expect(source).toContain("buzzWhenInputDisabled");
  });
});
