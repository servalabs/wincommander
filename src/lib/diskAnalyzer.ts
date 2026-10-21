export interface DiskNode {
    name: string;
    fullPath: string;
    size: number;
    allocated: number | null;
    isDir: boolean;
    lastModified: string;
    fileCount: number;
    folderCount: number;
}

export interface LargeDiskItem extends DiskNode {
    itemType: string;
    cleanupHint: string;
    risk: string;
}

export interface DiskScanMeta {
    scanRoot: string;
    totalSize: number;
    freeSpace: number;
    driveCapacity: number;
    fileCount: number;
    folderCount: number;
    skippedEntries?: number;
    complete?: boolean;
    skippedLinks?: number;
    truncated?: boolean;
    cancelled?: boolean;
}

export function formatDiskBytes(bytes: number): string {
    if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
    const units = ["B", "KB", "MB", "GB", "TB", "PB"];
    const index = Math.min(units.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)));
    return `${(bytes / 1024 ** index).toFixed(index >= 2 ? 1 : 0)} ${units[index]}`;
}

export function diskPercent(size: number, total: number): number {
    return total > 0 ? Math.min(100, Math.max(0, size / total * 100)) : 0;
}

export function normalizeDiskPath(path: string): string {
    return path.replace(/\//g, "\\").replace(/\\+$/, "").toLowerCase();
}

export function diskPathContains(parent: string, child: string): boolean {
    const root = normalizeDiskPath(parent);
    const candidate = normalizeDiskPath(child);
    return candidate === root || candidate.startsWith(`${root}\\`);
}

// A selected directory already includes its descendants; never count or delete them twice.
export function independentDiskSelection(items: DiskNode[]): DiskNode[] {
    const unique = new Map(items.map(item => [normalizeDiskPath(item.fullPath), item]));
    return [...unique.values()].filter(item => ![...unique.values()].some(parent =>
        parent !== item && parent.isDir && diskPathContains(parent.fullPath, item.fullPath)));
}

export function diskBreadcrumbs(root: string, path: string): string[] {
    const cleanRoot = root.replace(/\\+$/, "");
    if (!diskPathContains(root, path) || normalizeDiskPath(root) === normalizeDiskPath(path)) return [root];
    const suffix = path.slice(cleanRoot.length).replace(/^\\+/, "").split("\\").filter(Boolean);
    return [root, ...suffix.map((_, index) => `${cleanRoot}\\${suffix.slice(0, index + 1).join("\\")}`)];
}

export function diskNodeColor(node: DiskNode, index = 0): string {
    const colors = ["--color-accent", "--color-info", "--color-success", "--color-warning", "--color-text-secondary"];
    const extension = node.name.split(".").pop()?.toLowerCase();
    if (!node.isDir && extension && ["exe", "msi", "msix"].includes(extension)) return "var(--color-danger)";
    return `var(${colors[index % colors.length]})`;
}

export type LargeDiskFilter = "all" | "review" | "videos" | "archives" | "installers" | "folders";
export type DiskSort = "size" | "name" | "modified";

export function diskModifiedTime(value: string): number {
    const seconds = Number(value);
    if (/^\d+$/.test(value.trim())) return Number.isFinite(seconds) && seconds > 0 ? seconds * 1000 : 0;
    const parsed = Date.parse(value);
    return Number.isFinite(parsed) ? parsed : 0;
}

export function formatDiskModified(value: string): string {
    const timestamp = diskModifiedTime(value);
    return timestamp > 0 ? new Date(timestamp).toLocaleString() : "Unknown";
}

export function filterLargeDiskItems(items: LargeDiskItem[], filter: LargeDiskFilter, search: string, sort: DiskSort): LargeDiskItem[] {
    const query = search.trim().toLowerCase();
    const types: Partial<Record<LargeDiskFilter, string>> = { videos: "video", archives: "archive", installers: "installer" };
    return items.filter(item => (!query || `${item.name} ${item.fullPath}`.toLowerCase().includes(query)) &&
        (filter === "all" || (filter === "review" && ["review", "high", "danger"].includes(item.risk)) ||
            (filter === "folders" && item.isDir) || item.itemType.toLowerCase() === types[filter]))
        .sort((a, b) => sort === "name" ? a.name.localeCompare(b.name) : sort === "modified" ?
            diskModifiedTime(b.lastModified) - diskModifiedTime(a.lastModified) : b.size - a.size);
}
