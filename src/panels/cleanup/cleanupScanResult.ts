import type { CleanupCategory } from "./cleanupCategories";
import type { CardData } from "./useCleanupScan";

type ScanResponse = { success?: boolean; data?: unknown; error?: string };

export function readCleanupUserSummary(data: {
  id?: unknown; count?: unknown; items?: unknown; records?: unknown; error?: boolean;
  partial?: boolean; complete?: boolean; status?: string; message?: string;
  sampleCount?: unknown; previewLimit?: unknown; truncated?: unknown; coverage?: unknown; scanFacts?: unknown;
}): Pick<CardData, 'count' | 'items' | 'raw' | 'error'> {
  const records = Array.isArray(data.records) ? data.records : [];
  const grouped = data.id === 'ntUserTraces' && records.some(row => row && typeof row === 'object' && !Array.isArray(row) && Object.hasOwn(row, 'count'));
  const validGroup = grouped && records.every(row => row && typeof row === 'object' && !Array.isArray(row) && Number.isSafeInteger(row.count) && row.count >= 0);
  const groupTotal = validGroup ? records.reduce((sum, row) => sum + row.count, 0) : null;
  const previewOmitted = data.previewLimit === 0 || (data.truncated === true && data.sampleCount === 0);
  const incomplete = data.error === true || data.partial === true || data.complete === false ||
    ['error', 'partial', 'failed', 'unknown', 'unavailable'].includes(data.status ?? '') ||
    !Number.isSafeInteger(data.count) || (data.count as number) < 0 ||
    !Array.isArray(data.records) || data.records.some(row => !row || typeof row !== 'object' || Array.isArray(row)) ||
    (grouped && (!validGroup || !Number.isSafeInteger(groupTotal) || groupTotal !== data.count)) ||
    (data.count === 0 && records.length > 0 && !validGroup) ||
    (typeof data.count === 'number' && data.count > 0 && records.length === 0 && !previewOmitted) ||
    (data.sampleCount !== undefined && (!Number.isSafeInteger(data.sampleCount) || (data.sampleCount as number) < 0 || (Array.isArray(data.records) && data.sampleCount !== data.records.length))) ||
    (data.previewLimit !== undefined && (!Number.isSafeInteger(data.previewLimit) || (data.previewLimit as number) < 0 || (Array.isArray(data.records) && data.records.length > (data.previewLimit as number))));
  const scanFacts = data.scanFacts && typeof data.scanFacts === 'object' && !Array.isArray(data.scanFacts)
    ? Object.fromEntries(Object.entries(data.scanFacts).filter(([, value]) => typeof value === 'string' || typeof value === 'boolean' || (typeof value === 'number' && Number.isFinite(value)))) : {};
  if (incomplete) {
    // Keep safely decoded observations from a partial scan while leaving its
    // count unavailable, so these rows cannot authorize a cleanup.
    const observed = records.filter(row => row && typeof row === 'object' && !Array.isArray(row));
    const raw = observed.length || Object.keys(scanFacts).length ? {
      records: observed, scanFacts, partial: true,
      ...(typeof data.coverage === 'string' && data.coverage.trim() ? { coverage: data.coverage } : {}),
    } : undefined;
    return { count: -1, items: [], error: data.message || 'This user scan was incomplete.', ...(raw ? { raw } : {}) };
  }
  return {
    count: data.count as number,
    items: Array.isArray(data.items) ? data.items.filter((item): item is string => typeof item === 'string') : [],
    raw: {
      records: data.records,
      ...(Object.keys(scanFacts).length ? { scanFacts } : {}),
      ...(typeof data.coverage === 'string' && data.coverage.trim() ? { coverage: data.coverage } : {}),
      ...(Number.isSafeInteger(data.sampleCount) && (data.sampleCount as number) >= 0 ? { sampleCount: data.sampleCount } : {}),
      ...(Number.isSafeInteger(data.previewLimit) && (data.previewLimit as number) >= 0 ? { previewLimit: data.previewLimit } : {}),
      ...(typeof data.truncated === 'boolean' ? { truncated: data.truncated } : {}),
    },
  };
}

export function readCleanupScanResult(
  category: Pick<CleanupCategory, "id" | "extractPreview" | "requiredScanArrays">,
  response: ScanResponse,
): CardData {
  const failed = (message: string, raw?: Record<string, unknown>): CardData => ({
    count: -1, items: [], loading: false, clearing: false, error: message, ...(raw ? { raw } : {}),
  });
  if (!response.success || !response.data || typeof response.data !== "object") {
    return failed(response.error || "Scan failed.", response.data && typeof response.data === "object" && !Array.isArray(response.data) ? response.data as Record<string, unknown> : undefined);
  }
  const data = response.data as Record<string, unknown>;
  if (data.error === true || (typeof data.error === "string" && data.error.length > 0) || ["error", "failed", "unknown", "unavailable"].includes(String(data.status))) {
    return failed(typeof data.message === "string" ? data.message : "Scan failed.", data);
  }
  if (data.partial === true || data.status === "partial" || data.coverage === "partial" || data.complete === false || data.accessDenied === true) {
    return failed(typeof data.message === "string" ? data.message : "Scan was incomplete.", data);
  }
  if (category.requiredScanArrays?.length && !category.requiredScanArrays.some(key => Array.isArray(data[key]))) {
    return failed("Scan did not return its expected detail records.", data);
  }
  const firstDetailKey = category.requiredScanArrays?.find(key => Array.isArray(data[key]));
  const firstDetailRows = firstDetailKey ? data[firstDetailKey] : undefined;
  if (Array.isArray(firstDetailRows) && firstDetailRows.some(row => !row || typeof row !== "object" || Array.isArray(row))) {
    return failed("Scan returned malformed detail records.", data);
  }
  if (category.id === "ntfsJournals" && Array.isArray(data.journals) && data.journals.some(row => typeof (row as { present?: unknown }).present !== "boolean")) {
    return failed("Journal scan did not report whether each journal is active.", data);
  }
  let count: number;
  let items: string[];
  try {
    ({ count, items } = category.extractPreview(data));
  } catch {
    return failed("Scan details could not be read.", data);
  }
  if (!Number.isSafeInteger(count) || count < 0 || !Array.isArray(items)) {
    return failed("Scan returned an invalid count.", data);
  }
  if (category.id === "clipboardHistory") {
    const history = data.historyItems;
    if (!Array.isArray(history) || data.historyReadStatus !== "complete") {
      return failed("Clipboard history could not be read.", data);
    }
    count = history.length;
    items = history.map(() => "Clipboard item");
  }
  // The file readers and these three custom readers return one inventory row
  // per counted item. Their adapters otherwise default a missing total to 0.
  const fileKeys = category.requiredScanArrays?.includes("files") ? category.requiredScanArrays : undefined;
  const fullInventoryKey = fileKeys?.find(key => Array.isArray(data[key])) ??
    ({ crashDumps: "dumps", recallDb: "databases", recycleBin: "items", connectivityHistory: "entries" } as Record<string, string>)[category.id];
  if (fullInventoryKey) {
    const rows = data[fullInventoryKey];
    if (!Array.isArray(rows) || !Number.isSafeInteger(data.total) || (data.total as number) < 0) return failed("Scan did not return a valid inventory total.", data);
    if (category.id === "developerCaches") {
      if (!Number.isSafeInteger(data.previewLimit) || (data.previewLimit as number) < 0 || typeof data.truncated !== "boolean" ||
        rows.length !== Math.min(data.total as number, data.previewLimit as number) || data.truncated !== ((data.total as number) > rows.length)) {
        return failed("The inventory count disagrees with its bounded preview.", data);
      }
    } else if (rows.length !== data.total) {
      return failed("The inventory count disagrees with its detail records.", data);
    }
  }
  if (category.id === "psHistory") {
    const entries = data.entries;
    if (!Number.isSafeInteger(data.total) || (data.total as number) < 0 || !Array.isArray(entries) ||
      !Number.isSafeInteger(data.sampleCount) || data.sampleCount !== entries.length ||
      !Number.isSafeInteger(data.previewLimit) || (data.previewLimit as number) < 0 ||
      entries.length !== Math.min(data.total as number, data.previewLimit as number)) {
      return failed("Command history count or preview is incomplete.", data);
    }
  }
  if (category.id === "amcache" || category.id === "ntUserTraces") {
    const rows = category.id === "amcache" ? data.entries : data.sections;
    if (!Number.isSafeInteger(data.total) || !Array.isArray(rows) || rows.length === 0 || rows.some(row =>
      !row || typeof row !== "object" ||
      !Number.isSafeInteger((row as { count?: unknown }).count) ||
      ((row as { count: number }).count < 0)
    )) return failed("The category scan is incomplete.", data);
    const rowTotal = rows.reduce((sum, row) => sum + (row as { count: number }).count, 0);
    if (data.total !== count || rowTotal !== count) return failed("The scan total disagrees with its category counts. Scan again.", data);
    if (category.id === "ntUserTraces" && rows.some(row => {
      const section = row as { count: number; entries?: unknown };
      return !Array.isArray(section.entries) || section.entries.length !== section.count;
    })) return failed("The user activity count disagrees with its full detail records.", data);
  }
  if (category.id === "eventLogs") {
    if (typeof data.total !== "number" || !Number.isSafeInteger(data.total) || data.total < 0) return failed("Event log scan is incomplete.", data);
    if (!Array.isArray(data.logs) || data.logs.some(row => {
      if (!row || typeof row !== "object") return true;
      const value = (row as { entries?: unknown; count?: unknown }).entries ?? (row as { count?: unknown }).count;
      return !Number.isSafeInteger(value) || (value as number) < 0;
    })) return failed("Event log scan is incomplete.", data);
    if (data.total !== count) return failed("Event log total disagrees with its channel counts. Scan again.", data);
  }
  return { count, items, loading: false, clearing: false, raw: data };
}
