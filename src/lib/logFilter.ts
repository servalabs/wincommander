// Pure filtering for the in-app Error Center (src/panels/privacy/LogViewer.tsx).
// Kept out of the component so the level + source + search logic is unit-testable.

export interface LogRecord {
  date: string;
  timestamp: string;
  level: string;
  /** Origin of the record: "ui" (frontend), "core" (Free backend), "pro" (sidecar). */
  source: string;
  /** Operating system label. Older records do not carry this; the UI falls back to the current runtime OS. */
  os?: string;
  message: string;
  occurrences?: number;
  firstSeen?: string;
  lastSeen?: string;
}

/** Collapse consecutive identical (level+source+message) rows into one with a
 *  summed count + first/last timestamps. Records arrive newest-first. Pure → testable.
 *  Honors a wire-supplied `occurrences` (from Rust write-time summaries). */
export function groupLogRecords(records: LogRecord[]): LogRecord[] {
  const out: LogRecord[] = [];
  for (const r of records) {
    const prev = out[out.length - 1];
    if (prev && prev.level === r.level && prev.source === r.source && prev.message === r.message) {
      prev.occurrences = (prev.occurrences ?? 1) + (r.occurrences ?? 1);
      // newest-first: r is older than prev, so r's time is the earlier (first) bound
      prev.firstSeen = r.firstSeen ?? `${r.date} ${r.timestamp}`;
    } else {
      out.push({
        ...r,
        occurrences: r.occurrences ?? 1,
        firstSeen: r.firstSeen ?? `${r.date} ${r.timestamp}`,
        lastSeen: r.lastSeen ?? `${r.date} ${r.timestamp}`,
      });
    }
  }
  return out;
}

function _normalizedMessage(message: string): string {
  return message.trim().toLowerCase();
}

function _toEpochMs(date: string, timestamp: string): number {
  const parsed = Date.parse(`${date}T${timestamp}`);
  return Number.isNaN(parsed) ? 0 : parsed;
}

/** Collapse near-simultaneous rows that share level+source+exact message —
 *  e.g. React's internal error-boundary echo landing right next to our own
 *  componentDidCatch log line for the same crash, logged twice verbatim.
 *  Distinct events (different source/level, different message text even if
 *  they share a long common prefix, or the same wording recurring minutes
 *  apart) are never merged: the window is intentionally short (default 2s)
 *  and the message match is exact, not a fuzzy/prefix match — two different
 *  errors that happen to share a long prefix must stay distinct.
 *  Merged records sum `occurrences` and widen firstSeen/lastSeen to cover
 *  both, mirroring groupLogRecords, so no backend-summarized count is lost.
 *  Records arrive newest-first, same contract as groupLogRecords. */
export function dedupNearSimultaneous(
  records: LogRecord[],
  windowMs = 2000,
): LogRecord[] {
  const out: LogRecord[] = [];
  for (const r of records) {
    const prev = out[out.length - 1];
    const isNearDuplicate =
      prev &&
      prev.level === r.level &&
      prev.source === r.source &&
      _normalizedMessage(prev.message) === _normalizedMessage(r.message) &&
      Math.abs(_toEpochMs(prev.date, prev.timestamp) - _toEpochMs(r.date, r.timestamp)) <= windowMs;

    if (isNearDuplicate) {
      // Keep the richer (longer) message of the pair — componentDidCatch's
      // stack-annotated record over React's shorter internal echo, or
      // whichever call site logged more detail.
      if (r.message.length > prev!.message.length) {
        prev!.message = r.message;
      }
      // Sum occurrences instead of discarding the older record's count —
      // otherwise a backend-summarized "occurrences: 9" record gets dropped
      // in favor of a fresh "occurrences: 1" arrival and the UI shows x1.
      prev!.occurrences = (prev!.occurrences ?? 1) + (r.occurrences ?? 1);
      // newest-first: r is older than prev, so r's time is the earlier
      // (first) bound; prev's time (or its existing lastSeen) is the later
      // (last) bound.
      prev!.firstSeen = r.firstSeen ?? `${r.date} ${r.timestamp}`;
      prev!.lastSeen = prev!.lastSeen ?? `${prev!.date} ${prev!.timestamp}`;
      continue;
    }
    out.push({ ...r });
  }
  return out;
}

export const LEVEL_FILTERS = ["ALL", "ERROR_WARN", "ERROR", "WARN", "INFO"] as const;

export type LevelFilter = (typeof LEVEL_FILTERS)[number];
export type SourceFilter = "ALL" | "UI" | "CORE" | "PRO";

export function levelFilterToBackendLevels(level: LevelFilter): string[] | undefined {
  if (level === "ALL") return undefined;
  if (level === "ERROR_WARN") return ["error", "warn"];
  return [level.toLowerCase()];
}

/** Apply the level chip, source chip, and free-text search together. */
export function filterLogRecords(
  records: LogRecord[],
  level: LevelFilter,
  source: SourceFilter,
  search: string,
): LogRecord[] {
  const query = search.trim().toLowerCase();
  return records.filter((r) => {
    if (level === "ERROR_WARN" && !["ERROR", "WARN"].includes(r.level.toUpperCase())) return false;
    if (level !== "ALL" && level !== "ERROR_WARN" && r.level.toUpperCase() !== level) return false;
    if (source !== "ALL" && r.source.toUpperCase() !== source) return false;
    if (query && !r.message.toLowerCase().includes(query)) return false;
    return true;
  });
}
