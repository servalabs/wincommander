import { describe, expect, test } from "bun:test";
import { ariaSort, emptyTableMessage, filterAndSort } from "./privacyTableModel";

describe("privacy forensic table state model", () => {
  const rows = [
    { label: "USB device", severity: "high", count: 12 },
    { label: "Clipboard signal", severity: "medium", count: 2 },
    { label: "Print job", severity: "low", count: 7 },
  ];

  test("filters every supplied field case-insensitively", () => {
    expect(filterAndSort(rows, "HIGH", (row) => Object.values(row), (a, b) => a.count - b.count, true))
      .toEqual([rows[0]]);
    expect(filterAndSort(rows, "print", (row) => Object.values(row), (a, b) => a.count - b.count, true))
      .toEqual([rows[2]]);
  });

  test("sorts without mutating the backend result", () => {
    const original = [...rows];
    expect(filterAndSort(rows, "", (row) => Object.values(row), (a, b) => a.count - b.count, false)
      .map((row) => row.count)).toEqual([2, 7, 12]);
    expect(rows).toEqual(original);
  });

  test("distinguishes empty data from a zero-match filter", () => {
    expect(emptyTableMessage("events", 0, "")).toBe("No events recorded.");
    expect(emptyTableMessage("events", 3, "needle")).toBe("No events match “needle”.");
  });

  test("exposes the active sort direction", () => {
    expect(ariaSort(false, true)).toBe("none");
    expect(ariaSort(true, true)).toBe("descending");
    expect(ariaSort(true, false)).toBe("ascending");
  });
});
