import type { DiskFileTypes } from "@/lib/diskAnalyzer";

export type DiskTypeView = "category" | "extension";
type DiskTypeSort = "size" | "count" | "name";

export function getDiskTypeRows(result: DiskFileTypes | null, view: DiskTypeView, sort: DiskTypeSort): DiskFileTypes["categories"] {
    const data = view === "category" ? result?.categories : result?.extensions;
    const rows = Array.isArray(data) ? [...data] : [];
    return rows.sort((a, b) => sort === "name" ? a.category.localeCompare(b.category)
        : sort === "count" ? b.fileCount - a.fileCount || a.category.localeCompare(b.category)
            : b.logicalSize - a.logicalSize || a.category.localeCompare(b.category));
}
