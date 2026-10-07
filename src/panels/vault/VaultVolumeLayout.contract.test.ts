import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const layout = readFileSync("src/panels/vault/VaultVolumeLayout.css", "utf8");

describe("encrypted-volume table layout", () => {
  test("keeps headers and rows as a table when the Secure Storage card is narrow", () => {
    expect(layout).toContain(".encrypted-volumes-table {\n  width: 100%;\n  table-layout: fixed;\n}");
    expect(layout).not.toContain(".encrypted-volumes-table thead");
    expect(layout).not.toContain(".encrypted-volumes-table tbody tr");
    expect(layout).not.toContain("grid-template-columns");
  });

  test("wraps action controls and long text inside table cells instead of widening the card", () => {
    expect(layout).toContain(".encrypted-volumes-table .actions-cell > .vol-actions-group > div");
    expect(layout).toContain("flex-wrap: wrap");
    expect(layout).toContain("overflow-wrap: anywhere");
  });
});
