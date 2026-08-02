import { describe, expect, test } from "bun:test";
import { buildTraceView, humanizeTraceKey, paginateTraceRows, traceCellText, traceRowsToTsv } from "./traceTable";

describe("trace table view model", () => {
  test("exposes every scalar field from a forensic record as a table column", () => {
    const view = buildTraceView({
      files: [
        { name: "thumbcache_256.db", sizeKB: 42, modified: "2026-08-01", locked: false },
        { name: "iconcache_32.db", sizeKB: 7, modified: "2026-07-31", locked: true },
      ],
      total: 2,
      totalSizeMB: 0.05,
    }, []);

    expect(view.structured).toBe(true);
    expect(view.metadata).toEqual([
      { label: "Total", value: 2 },
      { label: "Total Size MB", value: 0.05 },
    ]);
    expect(view.datasets).toHaveLength(1);
    expect(view.datasets[0].columns).toEqual(["Name", "Size KB", "Modified", "Locked"]);
    expect(view.datasets[0].rows[1]["Name"]).toBe("iconcache_32.db");
  });

  test("expands nested sample arrays while retaining parent forensic context", () => {
    const view = buildTraceView({
      entries: [
        {
          category: "InventoryApplicationFile",
          count: 2,
          sample: [
            { id: "a1", name: "tool.exe", path: "C:\\Tools\\tool.exe" },
            { id: "a2", name: "agent.exe", path: "C:\\Tools\\agent.exe" },
          ],
        },
      ],
      total: 2,
    }, []);

    const samples = view.datasets.find((dataset) => dataset.id === "entries.sample");
    expect(samples == null).toBe(false);
    expect(samples?.columns).toEqual(["Category", "Count", "ID", "Name", "Path"]);
    expect(samples?.rows[0]).toMatchObject({
      Category: "InventoryApplicationFile",
      Count: 2,
      ID: "a1",
      Name: "tool.exe",
      Path: "C:\\Tools\\tool.exe",
    });
  });

  test("turns legacy flattened strings into useful fallback columns", () => {
    const view = buildTraceView(undefined, [
      "[NetworkList] Office Wi-Fi",
      "Z: → \\\\server\\share",
      "Application: 142 entries",
    ]);

    expect(view.structured).toBe(false);
    expect(view.datasets[0].columns).toEqual(["Source", "Entry", "Destination", "Name", "Value"]);
    expect(view.datasets[0].rows[0]).toEqual({ Source: "NetworkList", Entry: "Office Wi-Fi" });
    expect(view.datasets[0].rows[1]).toEqual({ Source: "Z:", Destination: "\\\\server\\share" });
  });

  test("expands scalar arrays into forensic columns instead of one generic Value column", () => {
    const view = buildTraceView({
      paths: [
        "C:\\Users\\Audit\\Recent\\report.docx.lnk",
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\RunMRU",
        "2026-08-01 21:14:52 — audit-tool.exe launched",
      ],
    }, []);

    expect(view.datasets[0].columns).toEqual([
      "Name",
      "Directory",
      "Extension",
      "Path",
      "Hive",
      "Key",
      "Timestamp",
      "Entry",
    ]);
    expect(view.datasets[0].rows[0]).toMatchObject({
      Name: "report.docx.lnk",
      Extension: "lnk",
      Path: "C:\\Users\\Audit\\Recent\\report.docx.lnk",
    });
  });

  test("never falls back to a one-column forensic dataset", () => {
    const plainText = buildTraceView({ entries: ["unstructured artifact"] }, []);
    const scalars = buildTraceView({ values: [42, false, null] }, []);

    expect(plainText.datasets[0].columns).toEqual(["Type", "Entry"]);
    expect(plainText.datasets[0].rows[0]).toEqual({ Type: "Text record", Entry: "unstructured artifact" });
    expect(scalars.datasets[0].columns).toEqual(["Type", "Value"]);
    expect(scalars.datasets[0].rows.map((row) => row.Type)).toEqual(["Number", "Boolean", "Empty"]);
  });

  test("paginates large datasets deterministically and clamps stale pages", () => {
    const rows = Array.from({ length: 205 }, (_, index) => ({ index }));
    const second = paginateTraceRows(rows, 1);
    expect(second.page).toBe(1);
    expect(second.totalPages).toBe(3);
    expect(second.startIndex).toBe(100);
    expect(second.rows).toHaveLength(100);
    expect(second.rows[0]).toEqual({ index: 100 });

    const clamped = paginateTraceRows(rows, 99);
    expect(clamped.page).toBe(2);
    expect(clamped.startIndex).toBe(200);
    expect(clamped.rows).toHaveLength(5);
  });

  test("exports the complete filtered row shape as spreadsheet-friendly TSV", () => {
    const tsv = traceRowsToTsv(
      ["Name", "Count", "Missing"],
      [{ Name: "Audit\tTool", Count: 2, Missing: null }, { Name: "Line 1\nLine 2", Count: 1 }],
    );

    expect(tsv).toBe("Name\tCount\tMissing\nAudit Tool\t2\t—\nLine 1 ↵ Line 2\t1\t—");
  });

  test("normalizes technical field labels and display values", () => {
    expect(humanizeTraceKey("totalSizeKB")).toBe("Total Size KB");
    expect(humanizeTraceKey("device_id")).toBe("Device ID");
    expect(traceCellText(false)).toBe("No");
    expect(traceCellText(null)).toBe("—");
  });
});
