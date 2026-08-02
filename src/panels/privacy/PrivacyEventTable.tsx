import { useMemo, useState, type ReactNode } from "react";

export interface PrivacyEventRow { id: string; cells: ReactNode[]; search: string; sort: string[]; }

export default function PrivacyEventTable({ title, columns, rows }: { title: string; columns: string[]; rows: PrivacyEventRow[] }) {
  const [filter, setFilter] = useState("");
  const [sortColumn, setSortColumn] = useState(0);
  const [descending, setDescending] = useState(true);
  const visible = useMemo(() => rows
    .filter((row) => row.search.toLowerCase().includes(filter.trim().toLowerCase()))
    .sort((a, b) => (descending ? -1 : 1) * (a.sort[sortColumn] ?? "").localeCompare(b.sort[sortColumn] ?? "", undefined, { numeric: true })),
    [descending, filter, rows, sortColumn]);
  return <section className="flex flex-col gap-2" aria-label={title}>
    <input type="search" aria-label={`Filter ${title}`} value={filter} onChange={(event) => setFilter(event.target.value)} placeholder="Filter events" className="min-w-44 self-end rounded border bg-transparent px-2 py-1 text-[11px]" />
    <div className="max-h-[220px] overflow-auto rounded border border-[var(--color-border)]" role="region" aria-label={`${title} data table`}>
      <table className="w-full text-left text-[11px]" aria-label={title}><thead className="sticky top-0 bg-[var(--color-bg-secondary)]"><tr>{columns.map((column, index) => <th scope="col" className="px-2 py-1.5" key={column}><button type="button" className="text-left" aria-label={`Sort ${title} by ${column}`} onClick={() => { if (sortColumn === index) setDescending((value) => !value); else { setSortColumn(index); setDescending(index === 0); } }}>{column}{sortColumn === index ? (descending ? " ↓" : " ↑") : ""}</button></th>)}</tr></thead><tbody>{visible.map((row) => <tr className="border-t border-[var(--color-border)]" key={row.id}>{row.cells.map((cell, index) => <td className="px-2 py-1.5 align-top" key={index}>{cell}</td>)}</tr>)}{visible.length === 0 && <tr><td colSpan={columns.length} className="px-2 py-3 text-center opacity-60">No matching events.</td></tr>}</tbody></table>
    </div>
    <span className="text-[10px] opacity-60">Showing {visible.length} of {rows.length} events.</span>
  </section>;
}
