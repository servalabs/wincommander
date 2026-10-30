import type { DiskNode, LargeDiskItem } from "../lib/diskAnalyzer";

const ROOT = "C:\\Audit";
const SCAN_ID = "synthetic-disk-scan";
const GIB = 1024 ** 3;
const MIB = 1024 ** 2;
const file = (entryId: number, folder: string, name: string, size: number): DiskNode => ({
  entryId, name, fullPath: `${folder}\\${name}`, size, allocated: null, isDir: false,
  lastModified: "2026-08-01T20:12:00Z", fileCount: 1, folderCount: 0,
});
const cases = [file(10, `${ROOT}\\Cases`, "case-archive.zip", 3 * GIB), file(11, `${ROOT}\\Cases`, "capture-session.mkv", 2 * GIB)];
const downloads = [file(12, `${ROOT}\\Downloads`, "setup.msi", GIB), file(13, `${ROOT}\\Downloads`, "photo.jpg", 60 * MIB)];
const caches = [file(14, `${ROOT}\\AppData`, "browser-cache.bin", 512 * MIB)];
const directory = (entryId: number, name: string, files: DiskNode[]): DiskNode => ({
  entryId, name, fullPath: `${ROOT}\\${name}`, size: files.reduce((total, row) => total + row.size, 0),
  allocated: null, isDir: true, lastModified: "2026-08-01T20:12:00Z", fileCount: files.length, folderCount: 0,
});
const children: DiskNode[] = [directory(1, "Cases", cases), directory(2, "Downloads", downloads), directory(3, "AppData", caches), file(4, ROOT, "evidence-image.iso", 6 * GIB)];
const files = [...cases, ...downloads, ...caches, children[3]];
const pathKey = (path: unknown) => String(path ?? ROOT).replace(/\\+$/, "").toLowerCase();

/** Synthetic data for the dedicated browser audit entrypoint, never native IPC. */
export function diskAnalyzerAuditResponse(command: string, args: Record<string, unknown> = {}): unknown {
  if (command === "run_disk_scan") return {
    scanId: SCAN_ID, scanRoot: ROOT, totalSize: files.reduce((sum, row) => sum + row.size, 0),
    freeSpace: 400 * GIB, driveCapacity: 1024 * GIB, fileCount: files.length, folderCount: 3,
    complete: true, skippedEntries: 0, skippedLinks: 0, truncated: false, cancelled: false,
    sizeBasis: "logical", allocatedSizeAvailable: false, wiztreeFound: true,
  };
  if (command === "get_disk_children") {
    const groups: Record<string, DiskNode[]> = { [pathKey(ROOT)]: children, [pathKey(`${ROOT}\\Cases`)]: cases, [pathKey(`${ROOT}\\Downloads`)]: downloads, [pathKey(`${ROOT}\\AppData`)]: caches };
    return structuredClone(groups[pathKey(args.path)] ?? []);
  }
  if (command === "get_disk_file_types") {
    const scope = pathKey(args.path);
    const scopedFiles = files.filter(row => pathKey(row.fullPath).startsWith(`${scope}\\`));
    const category = (name: string) => /\.(jpg|png)$/i.test(name) ? "images" : /\.mkv$/i.test(name) ? "videos" : /\.(zip|iso)$/i.test(name) ? "archives" : "unclassified";
    return { scanId: SCAN_ID, scopePath: String(args.path ?? ROOT), complete: true,
      categories: ["images", "videos", "audio", "documents", "code", "archives", "unclassified"].map(type => {
        const matches = scopedFiles.filter(row => category(row.name) === type);
        return { category: type, fileCount: matches.length, logicalSize: matches.reduce((sum, row) => sum + row.size, 0) };
      }) };
  }
  if (command === "get_large_disk_items") {
    const minimumSize = Number(args.minSizeBytes ?? 100 * MIB);
    const resultLimit = Math.min(5000, Math.max(1, Number(args.limit ?? 5000)));
    const items: LargeDiskItem[] = files.filter(row => row.size >= minimumSize).map(row => ({
      ...row, itemType: row.name.endsWith(".mkv") ? "Video" : row.name.endsWith(".jpg") ? "Image" : row.name.endsWith(".msi") ? "Installer" : "Archive",
      cleanupHint: "Review before deleting", risk: "medium",
    })).sort((a, b) => b.size - a.size);
    return { items: items.slice(0, resultLimit), matchedCount: items.length, matchedSize: items.reduce((sum, item) => sum + item.size, 0), resultLimit, limited: items.length > resultLimit, minimumSize, scanId: SCAN_ID };
  }
  if (command === "open_disk_item" || command === "cancel_disk_scan") return null;
  if (command === "get_disk_scan_progress") return { running: false, filesScanned: files.length, foldersScanned: 3, bytesScanned: files.reduce((sum, row) => sum + row.size, 0), skippedEntries: 0, cancelled: false };
  return undefined;
}
