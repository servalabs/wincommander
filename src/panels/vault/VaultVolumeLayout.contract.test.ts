import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const layout = readFileSync("src/panels/vault/VaultVolumeLayout.css", "utf8").replace(/\r\n/g, "\n");

describe("encrypted-volume table layout", () => {
  test("keeps headers and rows as a table when the Secure Storage card is narrow", () => {
    expect(layout).toContain(".encrypted-volumes-table {\n  width: 100%;\n  table-layout: fixed;\n}");
    expect(layout).not.toContain(".encrypted-volumes-table thead");
    expect(layout).not.toContain(".encrypted-volumes-table tbody tr");
    expect(layout).not.toContain("grid-template-columns");
  });

  test("keeps all action controls on one compact line", () => {
    expect(layout).toContain(".encrypted-volumes-table .actions-cell > .vol-actions-group > div");
    expect(layout).toContain("flex-wrap: nowrap");
    expect(layout).toContain("th:last-child { width: 156px; }");
    expect(layout).toContain("width: 24px;");
    expect(layout).toContain("gap: 2px;");
    expect(layout).toContain("overflow-wrap: anywhere");
  });

  test("truncates narrow-card paths instead of creating a tall row, retaining full path on hover", () => {
    expect(layout).toContain("@container vault-volumes (max-width: 560px)");
    expect(layout).toContain("white-space: nowrap;");
    expect(layout).toContain("text-overflow: ellipsis;");
    const panel = readFileSync("src/panels/vault/index.tsx", "utf8");
    expect(panel).toContain('className="truncate-path" title={vol.path || undefined}');
  });
});
