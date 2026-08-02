import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("Error Center multi-column viewer", () => {
  test("labels filters and exposes a real table header", async () => {
    const source = await Bun.file("src/panels/privacy/LogViewer.tsx").text();

    expect(source).toContain('role="group" aria-label="Log severity filters"');
    expect(source).toContain('role="group" aria-label="Log source filters"');
    expect(source).toContain("aria-pressed={level === l}");
    expect(source).toContain("aria-pressed={source === s}");
    expect(source).toContain('aria-label="Search log messages"');
    expect(source).toContain('role="table" aria-label="WinCommander diagnostic log records"');
    for (const heading of ["Date &amp; time", "Level", "Source", "OS", "Message"]) {
      expect(source).toContain(`role="columnheader">${heading}`);
    }
    expect(source).toContain('role="columnheader" className="log-viewer-count-header">Count');
  });

  test("makes every expandable row keyboard operable", async () => {
    const source = await Bun.file("src/panels/privacy/LogViewer.tsx").text();

    expect(source).toContain('role="row"');
    expect(source).toContain('role="cell"');
    expect(source).toContain("tabIndex={0}");
    expect(source).toContain("aria-expanded={expanded.has(key)}");
    expect(source).toContain('event.key === "Enter" || event.key === " "');
    expect(source).toContain("toggleRow(key)");
  });
});
