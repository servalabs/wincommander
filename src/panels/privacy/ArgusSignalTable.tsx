import { useMemo, useState } from "react";
import type { ArgusSignalEntry } from "@/hooks/useArgus";

type SortKey = "time" | "severity" | "class" | "magnitude";

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

function timeLabel(entry: ArgusSignalEntry): string {
  const start = entry.windowStart.replace("T", " ").slice(0, 16);
  const end = entry.windowEnd.slice(11, 16);
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
    const query = filter.trim().toLowerCase();
    const filtered = entries.filter((entry) => !query || [entry.kind, entry.class, entry.severity]
      .some((value) => value.toLowerCase().includes(query)));
    return [...filtered].sort((left, right) => {
      let comparison: number;
      if (sortKey === "time") comparison = Date.parse(left.windowStart) - Date.parse(right.windowStart);
      else if (sortKey === "severity") comparison = (SEVERITY_RANK[left.severity] ?? 0) - (SEVERITY_RANK[right.severity] ?? 0);
      else if (sortKey === "class") comparison = formatClass(left.class).localeCompare(formatClass(right.class));
      else comparison = left.magnitude - right.magnitude;
      return descending ? -comparison : comparison;
    });
  }, [descending, entries, filter, formatClass, sortKey]);

  const sort = (key: SortKey) => {
    if (key === sortKey) setDescending((value) => !value);
    else {
      setSortKey(key);
      setDescending(key !== "class");
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
        <input
          type="search"
          value={filter}
          onChange={(event) => setFilter(event.target.value)}
          aria-label={`Filter ${title}`}
          placeholder="Filter kind, class, severity"
          className="min-w-44 rounded border bg-transparent px-2 py-1 text-[11px]"
        />
      </div>
      <div className="max-h-[260px] overflow-auto rounded border border-[var(--color-border)]" role="region" aria-label={`${title} data grid`}>
        <table className="w-full text-left text-[11px]" aria-label={title}>
          <thead className="sticky top-0 bg-[var(--color-bg-secondary)] text-[var(--shield-text-muted)]">
            <tr>
              <th scope="col" className="px-2 py-1.5">{heading("Time", "time")}</th>
              <th scope="col" className="px-2 py-1.5">{heading("Severity", "severity")}</th>
              <th scope="col" className="px-2 py-1.5">Kind</th>
              <th scope="col" className="px-2 py-1.5">{heading("Class", "class")}</th>
              <th scope="col" className="px-2 py-1.5 text-right">{heading("Magnitude", "magnitude")}</th>
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
              <tr><td colSpan={5} className="px-2 py-3 text-center opacity-60">No matching signals.</td></tr>
            )}
          </tbody>
        </table>
      </div>
      <span className="text-[10px] opacity-60">Showing {rows.length} of {entries.length} aggregate signals. No names, paths, or content are collected.</span>
    </section>
  );
}
