import type { RoutineCleanerCategory, RoutineCleanerItem, RoutineCleanerScan } from "../hooks/useBackend";

const TARGETS: Array<[string, RoutineCleanerCategory, string, string, number, number, boolean]> = [
  ["audit-edge-cache", "browsers", "Microsoft Edge cache", "C:\\Audit\\Browser\\Edge\\Cache", 8_388_608, 128, true],
  ["audit-firefox-thumbnails", "browsers", "Firefox thumbnail cache", "C:\\Audit\\Browser\\Firefox\\thumbnails", 1_310_720, 42, true],
  ["audit-app-cache", "applications", "Audit application cache", "C:\\Audit\\Cache", 2_621_440, 18, true],
  ["audit-shader-cache", "gaming", "DirectX shader cache", "C:\\Audit\\Gaming\\D3DCache", 67_108_864, 320, true],
  ["audit-launcher-logs", "gaming", "Game launcher logs", "C:\\Audit\\Gaming\\Launcher\\Logs", 786_432, 12, false],
  ["audit-sqlite-journals", "databases", "Orphaned SQLite journals", "C:\\Audit\\Databases", 3_670_016, 7, false],
];

/** Synthetic file previews are isolated to the development audit entrypoint. */
export function routineCleanerAuditResponse(): RoutineCleanerScan {
  const items: RoutineCleanerItem[] = TARGETS.map(([id, category, label, path, bytes, fileCount, recommended]) => ({
    id, category, label, path, bytes, fileCount, recommended, operation: "delete", truncated: false, description: null,
    pathDisplay: path, entryCount: fileCount, entriesLimit: 100, entriesTruncated: fileCount > 100,
    entries: Array.from({ length: Math.min(100, fileCount) }, (_, index) => ({
      id: `${id}-${index}`, path: `${path}\\cache-${String(index + 1).padStart(3, "0")}.bin`,
      actualSizeBytes: Math.floor(bytes / fileCount) + (index < bytes % fileCount ? 1 : 0), estimatedReclaimBytes: null,
    })),
  }));
  return { items, totalBytes: items.reduce((sum, item) => sum + item.bytes, 0), totalFiles: items.reduce((sum, item) => sum + item.fileCount, 0), skippedTargets: 0, cancelled: false };
}
