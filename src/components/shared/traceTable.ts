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
        scalarRows.map((item) => ({ ...context, Value: item })),
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

  return { Entry: item };
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
  return columns;
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
