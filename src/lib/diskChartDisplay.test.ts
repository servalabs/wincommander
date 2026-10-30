import { expect, test } from "bun:test";
import { prepareDiskChart, isDiskRemainder, diskChartCoverage } from "./diskChartDisplay";
import type { DiskChartNode } from "./diskAnalyzerLayout";

function node(id: number, size: number, isDir = false): DiskChartNode {
    return { entryId: id, name: `item${id}`, fullPath: `C:\\item${id}`, size, isDir, allocated: null, lastModified: "", fileCount: 0, folderCount: 0 };
}

test("thousands of tiny items retain their exact bytes and counts in Other instead of disappearing", () => {
    const items = [node(1, 1000000), ...Array.from({ length: 10000 }, (_, index) => node(index + 2, 1))];
    const display = prepareDiskChart(items, 1, "treemap", 800, 500, "C:\\");
    expect(display.nodes.reduce((sum, item) => sum + item.size, 0)).toBe(1010000);
    expect(display.remainders).toHaveLength(1);
    const remainder = display.remainders[0];
    expect([remainder.size, remainder.fileCount, remainder.remainder?.entryCount]).toEqual([10000, 10000, 10000]);
    expect(isDiskRemainder(remainder)).toBe(true);
    expect(display.renderedNodes <= 60).toBe(true);
    expect(items.every(item => !item.remainder)).toBe(true);
});

test("zero-byte files remain inspectable in the Other legend without inventing chart area", () => {
    const display = prepareDiskChart([node(1, 50), node(2, 0)], 1, "sunburst", 400, 400, "C:\\");
    expect(display.nodes.map(item => item.size)).toEqual([50]);
    expect(display.remainders[0].fileCount).toBe(1);
    expect(display.remainders[0].size).toBe(0);
});

test("lazy folder details and rendering budgets keep accurate descendant totals", () => {
    const folder = { ...node(1, 900, true), fileCount: 9, folderCount: 2 };
    const display = prepareDiskChart([folder], 3, "treemap", 800, 500, "C:\\", 10);
    expect(display.remainders[0].size).toBe(900);
    expect(display.remainders[0].remainder?.entryCount).toBe(11);
    expect(display.remainders[0].remainder?.parentPath).toBe(folder.fullPath);
    const deep = { ...folder, size: 1000, fileCount: 1000, children: Array.from({ length: 1000 }, (_, index) => node(index + 10, 1)) };
    const limited = prepareDiskChart([deep], 3, "treemap", 800, 500, "C:\\", 10);
    expect(limited.renderedNodes <= 10).toBe(true);
    expect(limited.nodes[0].children?.reduce((sum, item) => sum + item.size, 0)).toBe(1000);
    expect(limited.remainders.length).toBeGreaterThan(0);
});

test("render-cap Other counts folder descendants without counting any visible sibling twice", () => {
    const folder = { ...node(900, 100, true), fileCount: 5, folderCount: 2 };
    const rows = [...Array.from({ length: 80 }, (_, index) => node(index, 1000)), folder];
    const display = prepareDiskChart(rows, 1, "sunburst", 400, 400, "C:\\");
    const remainder = display.remainders[0];
    expect(display.nodes.reduce((sum, item) => sum + item.size, 0)).toBe(80100);
    expect(remainder.fileCount + remainder.folderCount).toBe(remainder.remainder?.entryCount);
    expect(remainder.remainder?.directItems).toBe(21);
    expect(remainder.fileCount).toBe(25);
    expect(remainder.folderCount).toBe(3);
});

test("loaded children hidden by a small parent tile are disclosed as one exact Other group", () => {
    const child = node(2, 90);
    const folder = { ...node(1, 90, true), fileCount: 1, children: [child] };
    const groups = diskChartCoverage([], [{ node: folder, level: 1, drawn: true, collapsed: true }], "C:\\");
    expect(groups).toHaveLength(1);
    expect([groups[0].size, groups[0].fileCount, groups[0].folderCount]).toEqual([90, 1, 0]);
    expect(groups[0].remainder?.reason).toContain("too small");
    expect(groups[0].remainder?.parentPath).toBe(folder.fullPath);
});

test("tiny primitive coverage replaces nested groups without counting descendants twice", () => {
    const folder = { ...node(1, 90, true), fileCount: 3, folderCount: 1 };
    const existing = { ...node(-1, 30), fullPath: `other:${folder.fullPath}`, fileCount: 1,
        remainder: { parentPath: folder.fullPath, directItems: 1, entryCount: 1, reason: "Tiny child" } };
    const groups = diskChartCoverage([existing], [{ node: folder, level: 1, drawn: false, collapsed: false }, { node: existing, level: 2, drawn: false, collapsed: false }], "C:\\");
    expect(groups).toHaveLength(1);
    expect([groups[0].size, groups[0].fileCount, groups[0].folderCount]).toEqual([90, 3, 2]);
});

test("display omissions at the same location merge without duplicate Other keys", () => {
    const existing = { ...node(-1, 20), fullPath: "other:C:\\", fileCount: 2,
        remainder: { parentPath: "C:\\", directItems: 2, entryCount: 2, reason: "Grouped small files" } };
    const groups = diskChartCoverage([existing], [{ node: node(1, 3), level: 1, drawn: false, collapsed: false }], "C:\\");
    expect(groups).toHaveLength(1);
    expect([groups[0].size, groups[0].fileCount, groups[0].remainder?.entryCount]).toEqual([23, 3, 3]);
});
