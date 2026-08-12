export type TraceCell = string | number | boolean | null;

export interface TraceTableRow {
  [column: string]: TraceCell;
}

export interface TraceDataset {
  id: string;
  title: string;
  columns: string[];
  rows: TraceTableRow[];
}

export interface TraceMetadataItem {
  label: string;
  value: TraceCell;
}

export interface TraceViewModel {
  metadata: TraceMetadataItem[];
  datasets: TraceDataset[];
  structured: boolean;
}

export const TRACE_PAGE_SIZE = 100;

export interface TracePage<T> {
  page: number;
  pageSize: number;
  totalPages: number;
  startIndex: number;
  rows: T[];
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

const isScalar = (value: unknown): value is TraceCell =>
  value === null || ["string", "number", "boolean"].includes(typeof value);

export function humanizeTraceKey(key: string): string {
  const normalized = key
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .replace(/[_-]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();

  if (!normalized) return "Value";

  return normalized
    .split(" ")
    .map((word) => {
      const upper = word.toUpperCase();
      if (["CPU", "DNS", "ID", "IP", "KB", "MB", "NTFS", "PCA", "PID", "RDP", "SRUM", "USB", "WAL"].includes(upper)) {
        return upper;
      }
      return word.charAt(0).toUpperCase() + word.slice(1);
    })
    .join(" ");
}

function flattenRecord(
  record: Record<string, unknown>,
  prefix = "",
  includeCollectionSummary = true,
): TraceTableRow {
  const row: TraceTableRow = {};

  for (const [key, value] of Object.entries(record)) {
    const label = prefix ? `${prefix} / ${humanizeTraceKey(key)}` : humanizeTraceKey(key);
    if (isScalar(value)) {
      row[label] = value;
    } else if (Array.isArray(value)) {
      if (includeCollectionSummary) {
        row[label] = `${value.length} ${value.length === 1 ? "item" : "items"}`;
      }
    } else if (isRecord(value)) {
      Object.assign(row, flattenRecord(value, label, includeCollectionSummary));
    }
  }

  return row;
}

function datasetTitle(path: string[]): string {
  return path.length ? path.map(humanizeTraceKey).join(" / ") : "Details";
}

function mergeRows(
  target: Map<string, { path: string[]; rows: TraceTableRow[] }>,
  path: string[],
  rows: TraceTableRow[],
): void {
  if (!rows.length) return;
  const id = path.join(".") || "details";
  const existing = target.get(id);
  if (existing) {
    existing.rows.push(...rows);
  } else {
    target.set(id, { path, rows: [...rows] });
  }
}

function collectDatasets(
  value: unknown,
  path: string[],
  context: TraceTableRow,
  datasets: Map<string, { path: string[]; rows: TraceTableRow[] }>,
): void {
  if (Array.isArray(value)) {
    const objectRows = value.filter(isRecord);
    const scalarRows = value.filter(isScalar);

    if (objectRows.length) {
      mergeRows(
        datasets,
        path,
        objectRows.map((item) => ({ ...context, ...flattenRecord(item) })),
      );

      for (const item of objectRows) {
        const childContext = {
          ...context,
          ...flattenRecord(item, "", false),
        };
        for (const [key, child] of Object.entries(item)) {
          if (Array.isArray(child) || isRecord(child)) {
            collectDatasets(child, [...path, key], childContext, datasets);
          }
        }
      }
    } else if (scalarRows.length) {
      mergeRows(
        datasets,
        path,
        scalarRows.map((item) => ({
          ...context,
          ...(typeof item === "string"
            ? parseFallbackItem(item)
            : { Type: item === null ? "Empty" : humanizeTraceKey(typeof item), Value: item }),
        })),
      );
    }
    return;
  }

  if (!isRecord(value)) return;

  for (const [key, child] of Object.entries(value)) {
    if (Array.isArray(child) || isRecord(child)) {
      collectDatasets(child, [...path, key], context, datasets);
    }
  }
}

function parseFallbackItem(item: string): TraceTableRow {
  const deleted = item.match(/^(.*?)\s+\(deleted\s+(.+)\)$/i);
  if (deleted) return { Path: deleted[1], Deleted: deleted[2] };

  const sourceTag = item.match(/^\[([^\]]+)\]\s*(.+)$/);
  if (sourceTag) return { Source: sourceTag[1], Entry: sourceTag[2] };

  const arrow = item.match(/^(.*?)\s+→\s+(.+)$/);
  if (arrow) return { Source: arrow[1], Destination: arrow[2] };

  const separator = item.indexOf(": ");
  if (separator > 2) {
    return { Name: item.slice(0, separator), Value: item.slice(separator + 2) };
  }

  const registryPath = item.match(/^(HK(?:CU|LM|CR|U|CC)|HKEY_[A-Z_]+)\\(.+)$/i);
  if (registryPath) return { Hive: registryPath[1].toUpperCase(), Key: registryPath[2] };

  if (/^(?:[A-Z]:\\|\\\\)/i.test(item)) {
    const lastSeparator = Math.max(item.lastIndexOf("\\"), item.lastIndexOf("/"));
    const name = lastSeparator >= 0 ? item.slice(lastSeparator + 1) : item;
    const directory = lastSeparator > 0 ? item.slice(0, lastSeparator) : item;
    const extensionMatch = name.match(/\.([^.]+)$/);
    return {
      ...(name ? { Name: name } : {}),
      Directory: directory,
      ...(extensionMatch ? { Extension: extensionMatch[1].toLocaleLowerCase() } : {}),
      Path: item,
    };
  }

  const timestamped = item.match(/^(\d{4}-\d{2}-\d{2}(?:[ T]\d{2}:\d{2}(?::\d{2})?)?)\s+(?:[|–—-])\s+(.+)$/);
  if (timestamped) return { Timestamp: timestamped[1], Entry: timestamped[2] };

  return { Type: "Text record", Entry: item };
}

const TRACE_COLUMN_PRIORITY: Array<[RegExp, number]> = [
  [/^(name|file name|entry)$/i, 0],
  [/(^|\s)(size|bytes|length|capacity)/i, 1],
  [/(time|date|modified|created|last seen|last run|timestamp)/i, 2],
  [/(^|\s)(path|directory|location|destination)$/i, 3],
];

function columnPriority(column: string): number {
  return TRACE_COLUMN_PRIORITY.find(([pattern]) => pattern.test(column))?.[1] ?? 10;
}

function orderedColumns(rows: TraceTableRow[]): string[] {
  const columns: string[] = [];
  const seen = new Set<string>();
  for (const row of rows) {
    for (const column of Object.keys(row)) {
      if (!seen.has(column)) {
        seen.add(column);
        columns.push(column);
      }
    }
  }
  // Consistent leading columns make every cleanup detail table easier to scan.
  // Columns with no variation add the least information and belong at the end.
  return columns.sort((left, right) => {
    const values = (column: string) => rows.map((row) => traceCellText(row[column]));
    const isLowInformation = (column: string) => {
      const distinct = new Set(values(column));
      return distinct.size <= 1 || [...distinct].every((value) => value === "—" || value === "0");
    };
    const lowInformationOrder = Number(isLowInformation(left)) - Number(isLowInformation(right));
    if (lowInformationOrder) return lowInformationOrder;
    const semanticOrder = columnPriority(left) - columnPriority(right);
    return semanticOrder || left.localeCompare(right, undefined, { sensitivity: "base" });
  });
}

/** Columns that contain no useful distinction obscure the information that does. */
export function visibleTraceColumns(columns: string[], rows: TraceTableRow[]): string[] {
  return columns.filter((column) => rows.some((row) => {
    const value = traceCellText(row[column]).trim();
    return value !== "—" && value !== "0";
  }));
}

export function formatTraceCell(value: TraceCell | undefined, column?: string): string {
  const text = traceCellText(value);
  if (text === "—") return text;
  if (typeof value === "number" && /(?:size|bytes|length)\s*kb\b/i.test(column ?? "")) {
    const megabytes = value / 1024;
    return megabytes >= 1000 ? `${(megabytes / 1000).toFixed(2)} GB` : `${megabytes.toFixed(2)} MB`;
  }
  if (typeof value === "string" && /(?:time|date|modified|created|last seen|last run|timestamp)/i.test(column ?? "")) {
    const parsed = new Date(value);
    if (!Number.isNaN(parsed.getTime())) {
      const hasTime = /[T\s]\d{1,2}:\d{2}/.test(value);
      return parsed.toLocaleString(undefined, {
        dateStyle: "medium",
        ...(hasTime ? { timeStyle: "short", hour12: true } : {}),
      });
    }
  }
  return text;
}

export function buildTraceView(rawData: unknown, fallbackItems: string[]): TraceViewModel {
  const rawDatasets = new Map<string, { path: string[]; rows: TraceTableRow[] }>();
  collectDatasets(rawData, [], {}, rawDatasets);

  const metadataRow = isRecord(rawData) ? flattenRecord(rawData, "", false) : {};
  const metadata = Object.entries(metadataRow).map(([label, value]) => ({ label, value }));

  const datasets: TraceDataset[] = Array.from(rawDatasets.entries()).map(([id, entry]) => ({
    id,
    title: datasetTitle(entry.path),
    columns: orderedColumns(entry.rows),
    rows: entry.rows,
  }));

  if (!datasets.length && fallbackItems.length) {
    const rows = fallbackItems.map(parseFallbackItem);
    datasets.push({
      id: "entries",
      title: "Entries",
      columns: orderedColumns(rows),
      rows,
    });
  }

  return {
    metadata,
    datasets,
    structured: rawDatasets.size > 0,
  };
}

export function traceCellText(value: TraceCell | undefined): string {
  if (value === null || value === undefined || value === "") return "—";
  if (typeof value === "boolean") return value ? "Yes" : "No";
  return String(value);
}

export function traceRowsToTsv(columns: string[], rows: TraceTableRow[]): string {
  const cleanCell = (value: TraceCell | undefined) => traceCellText(value)
    .replace(/\t/g, " ")
    .replace(/\r?\n/g, " ↵ ");
  return [
    columns.map((column) => cleanCell(column)).join("\t"),
    ...rows.map((row) => columns.map((column) => cleanCell(row[column])).join("\t")),
  ].join("\n");
}

export function paginateTraceRows<T>(rows: T[], requestedPage: number, pageSize = TRACE_PAGE_SIZE): TracePage<T> {
  const normalizedPageSize = Number.isFinite(pageSize) && pageSize > 0 ? Math.floor(pageSize) : TRACE_PAGE_SIZE;
  const totalPages = Math.max(1, Math.ceil(rows.length / normalizedPageSize));
  const normalizedPage = Number.isFinite(requestedPage) ? Math.max(0, Math.floor(requestedPage)) : 0;
  const page = Math.min(normalizedPage, totalPages - 1);
  const startIndex = page * normalizedPageSize;
  return {
    page,
    pageSize: normalizedPageSize,
    totalPages,
    startIndex,
    rows: rows.slice(startIndex, startIndex + normalizedPageSize),
  };
}
