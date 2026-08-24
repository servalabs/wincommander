import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

const dashboard = await Bun.file("src/panels/dashboard/index.tsx").text();
const css = await Bun.file("src/panels/dashboard/index.css").text();

describe("dashboard alert drawers", () => {
  test("uses one shared drawer state for System Info and Network Traffic", () => {
    expect(dashboard).toContain('useState<"system" | "network" | null>(null)');
    expect(dashboard).toContain('alertOpen={openAlertDrawer === "system"}');
    expect(dashboard).toContain('drawerOpen={openAlertDrawer === "network"}');
    expect(dashboard).toContain('setOpenAlertDrawer(open ? id : (current) => current === id ? null : current)');
  });

  test("keeps Network Traffic alert controls inside the responsive drawer", () => {
    expect(css).toContain('grid-template-columns: 24px minmax(52px, 1fr);');
    expect(css).toContain('grid-column: 2;');
    expect(css).toContain('flex-wrap: wrap;');
    expect(css).not.toContain('grid-template-columns: 24px minmax(52px, 1fr) auto;');
  });
});
