import { describe, expect, test } from "bun:test";
import { diskBreadcrumbs, diskModifiedTime, diskPathContains, filterLargeDiskItems, formatDiskBytes, independentDiskSelection, type DiskNode, type LargeDiskItem } from "./diskAnalyzer";
import { diskArcPath, diskSunburst, diskTreemap, type DiskChartNode } from "./diskAnalyzerLayout";

function node(fullPath: string, size: number, isDir = false): DiskNode {
    return { name: fullPath.split("\\").pop()!, fullPath, size, isDir, allocated: null, lastModified: "", fileCount: 0, folderCount: 0 };
}

describe("disk selection accounting", () => {
    test("counts selected parent and nested descendants once, without swallowing sibling prefixes", () => {
        const items = [node("C:\\Cache", 300, true), node("c:\\cache\\child.bin", 100), node("C:\\Cache2\\keep.bin", 200)];
        expect(independentDiskSelection(items).map(item => item.fullPath)).toEqual(["C:\\Cache", "C:\\Cache2\\keep.bin"]);
        expect(independentDiskSelection(items).reduce((sum, item) => sum + item.size, 0)).toBe(500);
    });
    test("deduplicates case-insensitive Windows paths and recognizes drive roots", () => {
        expect(independentDiskSelection([node("C:\\a.bin", 20), node("c:\\A.bin", 20)])).toHaveLength(1);
        expect(diskPathContains("C:\\", "c:\\a.bin")).toBe(true);
        expect(diskPathContains("C:\\", "D:\\a.bin")).toBe(false);
    });
    test("builds real intermediate breadcrumbs when a chart skips a hierarchy level", () => {
        expect(diskBreadcrumbs("C:\\", "C:\\Users\\Person\\Cache")).toEqual(["C:\\", "C:\\Users", "C:\\Users\\Person", "C:\\Users\\Person\\Cache"]);
        expect(diskBreadcrumbs("C:\\Users", "C:\\UsersOther")).toEqual(["C:\\Users"]);
    });
});

describe("disk chart accounting", () => {
    test("treemap covers the available area in proportion to actual logical sizes", () => {
        const tiles = diskTreemap([node("C:\\a", 300), node("C:\\b", 100), node("C:\\empty", 0)], 800, 500, 1);
        expect(tiles).toHaveLength(2);
        expect(Math.abs(tiles.reduce((sum, tile) => sum + tile.width * tile.height, 0) - 400000) < 0.0001).toBe(true);
        expect(Math.abs(tiles[0].width * tiles[0].height - 300000) < 0.0001).toBe(true);
        expect(Math.abs(tiles[1].width * tiles[1].height - 100000) < 0.0001).toBe(true);
        expect(tiles.every(tile => tile.x >= 0 && tile.y >= 0 && tile.x + tile.width <= 800.0001 && tile.y + tile.height <= 500.0001)).toBe(true);
    });
    test("sunburst retains unexpanded bytes instead of inflating the known child", () => {
        const parent: DiskChartNode = { ...node("C:\\parent", 400, true), children: [node("C:\\parent\\child", 100)] };
        const arcs = diskSunburst([parent], 2);
        expect(Math.abs(arcs[0].end - arcs[0].start - 2 * Math.PI) < 0.0001).toBe(true);
        expect(Math.abs(arcs[1].end - arcs[1].start - Math.PI / 2) < 0.0001).toBe(true);
        expect(diskSunburst([parent], 1)).toHaveLength(1);
    });
    test("zero-size results remain empty and a full-circle arc stays finite", () => {
        expect(diskTreemap([node("C:\\empty", 0)], 800, 500, 3)).toEqual([]);
        expect(diskSunburst([node("C:\\empty", 0)], 3)).toEqual([]);
        expect(/NaN|Infinity/.test(diskArcPath(0, Math.PI * 2, 62, 236))).toBe(false);
    });
});

test("large-file search combines type filters and size sorting", () => {
    const items: LargeDiskItem[] = [
        { ...node("C:\\Videos\\large.mp4", 500), itemType: "Video", cleanupHint: "", risk: "personal" },
        { ...node("C:\\Videos\\small.mp4", 100), itemType: "Video", cleanupHint: "", risk: "personal" },
        { ...node("C:\\Downloads\\archive.zip", 700), itemType: "Archive", cleanupHint: "", risk: "review" },
    ];
    expect(filterLargeDiskItems(items, "videos", "videos", "size").map(item => item.size)).toEqual([500, 100]);
    expect(filterLargeDiskItems(items, "review", "downloads", "name").map(item => item.name)).toEqual(["archive.zip"]);
    expect(formatDiskBytes(1024 ** 3)).toBe("1.0 GB");
});

test("backend Unix-second modification times sort chronologically", () => {
    expect(diskModifiedTime("1792540800")).toBe(1792540800000);
    expect(diskModifiedTime("2026-10-21T00:00:00Z")).toBe(Date.parse("2026-10-21T00:00:00Z"));
    expect(diskModifiedTime("")).toBe(0);
    expect(diskModifiedTime("0")).toBe(0);
});

test("higher risk filter includes the backend high-risk classification", () => {
    const item: LargeDiskItem = { ...node("C:\\Windows\\large.log", 500), itemType: "Temp / log", cleanupHint: "", risk: "high" };
    expect(filterLargeDiskItems([item], "review", "", "size")).toEqual([item]);
});
