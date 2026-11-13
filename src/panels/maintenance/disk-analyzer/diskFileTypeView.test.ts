import { expect, test } from "bun:test";
import type { DiskFileTypes } from "@/lib/diskAnalyzer";
import { getDiskTypeRows } from "./diskFileTypeView";

test("category and extension views retain their own counts, sizes and ordering", () => {
    const result: DiskFileTypes = {
        scanId: "scan", scopePath: "C:\\", complete: false,
        categories: [{ category: "Documents", fileCount: 3, logicalSize: 30 }, { category: "Images", fileCount: 1, logicalSize: 80 }],
        extensions: [{ category: ".pdf", fileCount: 3, logicalSize: 30 }, { category: ".png", fileCount: 1, logicalSize: 80 }],
    };
    expect(getDiskTypeRows(result, "category", "size").map(row => row.category)).toEqual(["Images", "Documents"]);
    expect(getDiskTypeRows(result, "extension", "count").map(row => row.category)).toEqual([".pdf", ".png"]);
    expect(getDiskTypeRows(result, "category", "name")[0].logicalSize).toBe(30);
    expect(result.categories[0].category).toBe("Documents");
});

test("an older result still shows categories when extension data is unavailable", () => {
    const oldResult = { categories: [{ category: "Other", fileCount: 1, logicalSize: 5 }] } as DiskFileTypes;
    expect(getDiskTypeRows(oldResult, "category", "size").length).toBe(1);
    expect(getDiskTypeRows(oldResult, "extension", "size").length).toBe(0);
});
