import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

describe("Debug Tools sidebar icon", () => {
  test("uses the bug icon in the footer and panel manifest", () => {
    const sidebar = readFileSync("src/components/Sidebar.tsx", "utf8");
    const panels = readFileSync("src/types/panels.ts", "utf8");

    expect(sidebar).toContain('<Icon icon="bug" size={17} className="item-icon" />');
    expect(panels).toMatch(/id: "dev",\s+label: "Dev Tools",\s+icon: "bug"/);
  });
});
