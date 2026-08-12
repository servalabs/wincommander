import { useMemo, useState } from "react";
import type { ArgusSignalEntry } from "@/hooks/useArgus";
import { ariaSort, emptyTableMessage, filterAndSort } from "./privacyTableModel";

type SortKey = "time" | "severity" | "kind" | "class" | "magnitude";

const SEVERITY_RANK: Record<string, number> = {
  critical: 4,
  high: 3,
  medium: 2,
  warn: 2,
  low: 1,
  info: 0,
};

function severityColor(severity: string): string {
  if (severity === "critical" || severity === "high") return "var(--color-danger, #f87171)";
  if (severity === "warn" || severity === "medium") return "var(--color-warning, #fbbf24)";
  return "var(--color-text-muted)";
}

/**
 * Fleet/Pro can return a persisted pre-schema signal without window bounds.
 * Treat that record as an unknown time instead of letting one malformed
 * historical row take down the entire Privacy panel.
 */
export function timeLabel(entry: Pick<ArgusSignalEntry, "windowStart" | "windowEnd">): string {
  const startValue = typeof entry.windowStart === "string" ? entry.windowStart : "";
  const endValue = typeof entry.windowEnd === "string" ? entry.windowEnd : "";
  const start = startValue.replace("T", " ").slice(0, 16);
  const end = endValue.slice(11, 16);
  if (!start) return end || "Unknown time";
  return end ? `${start}–${end}` : start;
}

export interface ArgusSignalTableProps {
  entries: ArgusSignalEntry[];
  title: string;
  formatClass?: (value: string) => string;
  formatMagnitude: (entry: ArgusSignalEntry) => string;
}

/** A privacy-safe, inspectable view of aggregate Argus signals. */
export default function ArgusSignalTable({
  entries,
  title,
  formatClass = (value) => value,
  formatMagnitude,
}: ArgusSignalTableProps) {
  const [filter, setFilter] = useState("");
  const [sortKey, setSortKey] = useState<SortKey>("time");
  const [descending, setDescending] = useState(true);

  const rows = useMemo(() => {
    return filterAndSort(entries, filter, (entry) => [
      entry.windowStart,
      entry.windowEnd,
      entry.severity,
      entry.kind,
      formatClass(entry.class),
      entry.magnitude,
      formatMagnitude(entry),
    ], (left, right) => {
      let comparison: number;
      if (sortKey === "time") comparison = Date.parse(left.windowStart) - Date.parse(right.windowStart);
      else if (sortKey === "severity") comparison = (SEVERITY_RANK[left.severity] ?? 0) - (SEVERITY_RANK[right.severity] ?? 0);
      else if (sortKey === "kind") comparison = left.kind.localeCompare(right.kind);
      else if (sortKey === "class") comparison = formatClass(left.class).localeCompare(formatClass(right.class));
      else comparison = left.magnitude - right.magnitude;
      return comparison;
    }, descending);
  }, [descending, entries, filter, formatClass, formatMagnitude, sortKey]);

  const sort = (key: SortKey) => {
    if (key === sortKey) setDescending((value) => !value);
    else {
      setSortKey(key);
      setDescending(key !== "class" && key !== "kind");
    }
  };
  const heading = (label: string, key: SortKey) => (
    <button type="button" onClick={() => sort(key)} aria-label={`Sort ${title} by ${label}`} className="text-left font-medium">
      {label}{sortKey === key ? (descending ? " ↓" : " ↑") : ""}
    </button>
  );

  return (
    <section className="flex flex-col gap-2" aria-label={title}>
      <div className="flex items-center justify-between gap-2 flex-wrap">
        <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)]">
          {title} ({entries.length})
        </span>
        <div className="flex min-w-0 flex-1 items-center justify-end gap-2">
          <input
            type="search"
            value={filter}
            onChange={(event) => setFilter(event.target.value)}
            aria-label={`Filter ${title}`}
            placeholder="Filter every column"
            className="min-w-0 flex-1 rounded border bg-transparent px-2 py-1 text-[11px] sm:max-w-56"
          />
          {filter && <button type="button" className="rounded border px-2 py-1 text-[11px]" aria-label={`Clear ${title} filter`} onClick={() => setFilter("")}>Clear</button>}
        </div>
      </div>
      <div className="max-h-[260px] overflow-auto rounded border border-[var(--color-border)]" role="region" aria-label={`${title} data grid`}>
        <table className="w-full text-left text-[11px]" aria-label={title}>
          <caption className="sr-only">{title}</caption>
          <thead className="sticky top-0 bg-[var(--color-bg-secondary)] text-[var(--shield-text-muted)]">
            <tr>
              <th scope="col" aria-sort={ariaSort(sortKey === "time", descending)} className="px-2 py-1.5">{heading("Time", "time")}</th>
              <th scope="col" aria-sort={ariaSort(sortKey === "severity", descending)} className="px-2 py-1.5">{heading("Severity", "severity")}</th>
              <th scope="col" aria-sort={ariaSort(sortKey === "kind", descending)} className="px-2 py-1.5">{heading("Kind", "kind")}</th>
              <th scope="col" aria-sort={ariaSort(sortKey === "class", descending)} className="px-2 py-1.5">{heading("Class", "class")}</th>
              <th scope="col" aria-sort={ariaSort(sortKey === "magnitude", descending)} className="px-2 py-1.5 text-right">{heading("Magnitude", "magnitude")}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((entry, index) => (
              <tr key={`${entry.windowStart}-${entry.class}-${entry.kind}-${index}`} className="border-t border-[var(--color-border)]">
                <td className="whitespace-nowrap px-2 py-1.5 font-mono opacity-70">{timeLabel(entry)}</td>
                <td className="px-2 py-1.5 font-mono font-medium" style={{ color: severityColor(entry.severity) }}>{entry.severity.toUpperCase()}</td>
                <td className="px-2 py-1.5 font-mono">{entry.kind}</td>
                <td className="px-2 py-1.5">{formatClass(entry.class)}</td>
                <td className="px-2 py-1.5 text-right font-mono font-medium">{formatMagnitude(entry)}</td>
              </tr>
            ))}
            {rows.length === 0 && (
              <tr><td colSpan={5} className="px-2 py-3 text-center opacity-60">{emptyTableMessage("signals", entries.length, filter)}</td></tr>
            )}
          </tbody>
        </table>
      </div>
      <span className="text-[10px] opacity-60" role="status" aria-live="polite">Showing {rows.length} of {entries.length} aggregate signals. No names, paths, or content are collected.</span>
    </section>
  );
}
