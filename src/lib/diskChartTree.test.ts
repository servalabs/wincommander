import { describe, expect, test } from "bun:test";
import type { DiskNode } from "./diskAnalyzer";
import { loadDiskChartTree } from "./diskChartTree";
import { diskSunburst } from "./diskAnalyzerLayout";

const folder: DiskNode = { entryId: 1, name: "Project", fullPath: "C:\\Project", size: 100, allocated: null, isDir: true, lastModified: "", fileCount: 1, folderCount: 1 };
const child: DiskNode = { ...folder, name: "Cache", fullPath: "C:\\Project\\Cache" };

describe("asynchronous disk chart expansion", () => {
  test("publishes a fresh expanded tree without mutating previously displayed nodes", async () => {
    const rows = [folder];
    const preview = rows.map(row => ({ ...row }));
    const result = await loadDiskChartTree(rows, 2, async () => [child]);
    expect(result.nodes === preview).toBe(false);
    expect(result.nodes[0] === folder).toBe(false);
    expect("children" in folder).toBe(false);
    expect("children" in preview[0]).toBe(false);
    expect(result.nodes[0].children?.[0].fullPath).toBe(child.fullPath);
    expect(diskSunburst(result.nodes, 2).map(arc => arc.level)).toEqual([1, 2]);
  });
  test("honors depth and request bounds while exposing incomplete chart detail", async () => {
    let calls = 0;
    const rows = Array.from({ length: 75 }, (_, index) => ({ ...folder, fullPath: `C:\\${index}` }));
    const result = await loadDiskChartTree(rows, 2, async () => { calls++; return []; });
    expect(calls).toBe(60);
    expect(result.limited).toBe(true);
    await loadDiskChartTree(rows, 1, async () => { throw new Error("Must not load below depth one"); });
    expect(calls).toBe(60);
  });
  test("retains the parent when child loading fails and stops obsolete requests", async () => {
    const failed = await loadDiskChartTree([folder], 2, async () => { throw new Error("Unavailable"); });
    expect(failed.nodes[0].size).toBe(100);
    expect(failed.limited).toBe(true);
    let calls = 0;
    await loadDiskChartTree([folder], 2, async () => { calls++; return []; }, () => false);
    expect(calls).toBe(0);
  });
});
