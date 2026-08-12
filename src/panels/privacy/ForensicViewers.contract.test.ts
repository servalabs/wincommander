import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("privacy forensic viewer contracts", () => {
  test("Argus signal feeds share a structured, filterable, sortable table", async () => {
    const table = await Bun.file("src/panels/privacy/ArgusSignalTable.tsx").text();

    expect(table).toContain('<table className="w-full text-left text-[11px]" aria-label={title}>');
    for (const heading of [
      '{heading("Time", "time")}',
      '{heading("Severity", "severity")}',
      '{heading("Kind", "kind")}',
      '{heading("Class", "class")}',
      '{heading("Magnitude", "magnitude")}',
    ]) expect(table).toContain(heading);
    expect(table).toContain('aria-label={`Filter ${title}`}');
    expect(table).toContain('placeholder="Filter every column"');
    expect(table).toContain('aria-sort={ariaSort(');
    expect(table).toContain('aria-live="polite"');
    expect(table).toContain('aria-label={`Sort ${title} by ${label}`}');
    expect(table).toContain('Showing {rows.length} of {entries.length} aggregate signals');

    for (const file of ["ArgusDlpSection.tsx", "ArgusPrintUsbSection.tsx", "ArgusTamperSection.tsx"]) {
      const source = await Bun.file(`src/panels/privacy/${file}`).text();
      expect(source).toContain('import ArgusSignalTable from \'./ArgusSignalTable\';');
      expect(source).toContain('<ArgusSignalTable entries={recent}');
      expect(source).not.toContain('[...recent].reverse().map');
    }
  });

  test("the app-usage list is a labelled multi-column dataset", async () => {
    const source = await Bun.file("src/panels/privacy/ProductivityTimeline.tsx").text();

    expect(source).toContain("import PrivacyEventTable from './PrivacyEventTable'");
    expect(source).toContain('title="Recent app-usage window records"');
    for (const heading of ["Time", "Category", "Active", "Idle", "Active share", "Top scores"]) {
      expect(source).toContain(`"${heading}"`);
    }
    expect(source).toContain('aria-label="Show app-usage table"');
    expect(source).toContain('aria-label="Show app-usage timeline"');
  });

  test("remaining privacy event feeds use the common filterable sortable table", async () => {
    const table = await Bun.file("src/panels/privacy/PrivacyEventTable.tsx").text();
    expect(table).toContain('aria-label={`Filter ${title}`}');
    expect(table).toContain('placeholder="Filter every column"');
    expect(table).toContain('aria-sort={ariaSort(');
    expect(table).toContain('aria-live="polite"');
    expect(table).toContain('aria-label={`Sort ${title} by ${column}`}');
    expect(table).toContain('<table className="w-full text-left text-[11px]" aria-label={title}>');
    for (const file of ["CanaryTokensSection.tsx", "DecoyMonitorSection.tsx", "PasteMonitorSection.tsx", "RansomwareMonitorSection.tsx", "RemoteAccessMonitorSection.tsx", "ScreenCaptureSection.tsx", "UsbDevicesSection.tsx"]) {
      const source = await Bun.file(`src/panels/privacy/${file}`).text();
      expect(source).toContain("PrivacyEventTable");
    }
  });
});
