import type { CleanupCategory } from "./cleanupCategories";
import type { CardData } from "./useCleanupScan";

type ScanResponse = { success?: boolean; data?: unknown; error?: string };

export function readCleanupUserSummary(data: {
  count?: unknown; items?: unknown; records?: unknown; error?: boolean;
  partial?: boolean; complete?: boolean; status?: string; message?: string;
}): Pick<CardData, 'count' | 'items' | 'raw' | 'error'> {
  const incomplete = data.error === true || data.partial === true || data.complete === false ||
    ['error', 'partial', 'failed', 'unknown', 'unavailable'].includes(data.status ?? '') ||
    !Number.isSafeInteger(data.count) || (data.count as number) < 0 ||
    ((data.count as number) > 0 && Array.isArray(data.records) && data.records.length === 0);
  if (incomplete) return { count: -1, items: [], error: data.message || 'This user scan was incomplete.' };
  return {
    count: data.count as number,
    items: Array.isArray(data.items) ? data.items.filter((item): item is string => typeof item === 'string') : [],
    raw: Array.isArray(data.records) ? { records: data.records } : undefined,
  };
}

export function readCleanupScanResult(
  category: Pick<CleanupCategory, "id" | "extractPreview" | "requiredScanArrays">,
  response: ScanResponse,
): CardData {
  const failed = (message: string): CardData => ({
    count: -1, items: [], loading: false, clearing: false, error: message,
  });
  if (!response.success || !response.data || typeof response.data !== "object") {
    return failed(response.error || "Scan failed.");
  }
  const data = response.data as Record<string, unknown>;
  if (data.error === true) {
    return failed(typeof data.message === "string" ? data.message : "Scan failed.");
  }
  if (data.partial === true || data.status === "partial" || data.complete === false) {
    return failed(typeof data.message === "string" ? data.message : "Scan was incomplete.");
  }
  if (category.requiredScanArrays?.length && !category.requiredScanArrays.some(key => Array.isArray(data[key]))) {
    return failed("Scan did not return its expected detail records.");
  }
  const { count, items } = category.extractPreview(data);
  if (!Number.isSafeInteger(count) || count < 0 || !Array.isArray(items)) {
    return failed("Scan returned an invalid count.");
  }
  if (data.total !== undefined && (typeof data.total !== "number" || !Number.isSafeInteger(data.total) || data.total < 0 || data.total !== count)) {
    return failed("Scan total disagrees with the card count. Scan again.");
  }
  if (count > 0 && items.length === 0 && data.truncated !== true) {
    return failed("Scan reported findings without any details. Scan again.");
  }
  if (category.id === "amcache") {
    const rows = data.entries;
    if (!Number.isSafeInteger(data.total) || !Array.isArray(rows) || rows.some(row =>
      !row || typeof row !== "object" ||
      !Number.isSafeInteger((row as { count?: unknown }).count) ||
      ((row as { count: number }).count < 0)
    )) return failed("App Launch Cache scan is incomplete.");
    const rowTotal = rows.reduce((sum, row) => sum + (row as { count: number }).count, 0);
    if (rowTotal !== count) return failed("App Launch Cache total disagrees with its category counts. Scan again.");
  }
  return { count, items, loading: false, clearing: false, raw: data };
}
