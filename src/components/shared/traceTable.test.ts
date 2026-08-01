import { describe, expect, test } from "bun:test";
import { buildTraceView, humanizeTraceKey, traceCellText } from "./traceTable";

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

  test("normalizes technical field labels and display values", () => {
    expect(humanizeTraceKey("totalSizeKB")).toBe("Total Size KB");
    expect(humanizeTraceKey("device_id")).toBe("Device ID");
    expect(traceCellText(false)).toBe("No");
    expect(traceCellText(null)).toBe("—");
  });
});
