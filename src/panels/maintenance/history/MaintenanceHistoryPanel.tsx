import { useMemo, useState } from "react";
import { Button } from "../../../components/ui/button";
import { Badge } from "../../../components/ui/badge";
import { Icon } from "../../../components/ui/icon";
import { useAppConfirm } from "../../../components/shared/AppConfirmDialog";
import { useMaintenanceHistory } from "../../../hooks/useMaintenanceHistory";
import { MAINTENANCE_TOOLS } from "../../../lib/maintenanceHistory";
import { formatBytes } from "../routineCleanerHelpers";

export default function MaintenanceHistoryPanel() {
  const history = useMaintenanceHistory();
  const requestConfirm = useAppConfirm();
  const [tool, setTool] = useState("all");
  const operations = useMemo(() => history.operations.filter(operation => tool === "all" || operation.tool === tool), [history.operations, tool]);
  const recovered = operations.reduce((total, operation) => total + (operation.bytesRecovered ?? 0), 0);

  const clear = async () => {
    if (await requestConfirm({ title: "Clear local operation history?", description: "This removes these display records. It does not undo completed operations or clear Windows logs.", confirmLabel: "Clear history" })) history.clearHistory();
  };

  return <section className="flex min-h-0 flex-col gap-4" aria-label="Local operation history">
    <div className="flex flex-wrap items-center justify-between gap-3 rounded-[var(--r-lg)] border border-[var(--border)] bg-[var(--surface)] p-4">
      <div><h2 className="text-base font-semibold">Operation history</h2><p className="mt-1 text-xs text-[var(--text-dim)]">The latest 100 recorded results on this device. Display records are not evidence receipts.</p></div>
      <div className="flex items-center gap-3"><span className="font-mono text-sm text-[var(--accent)]">{formatBytes(recovered)} reported removed</span><Button variant="outline" size="sm" disabled={!history.operations.length} onClick={() => void clear()}>Clear history</Button></div>
    </div>
    {!history.isPersistent && <p role="status" className="text-sm text-[var(--warn)]">Local storage is unavailable. New records will only remain for this session.</p>}
    <label className="flex items-center gap-3 text-xs text-[var(--text-dim)]">Filter by tool<select aria-label="Filter operation history by tool" value={tool} onChange={event => setTool(event.target.value)} className="rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface)] p-2 text-[var(--text)]"><option value="all">All tools</option>{MAINTENANCE_TOOLS.map(value => <option key={value} value={value}>{value.replaceAll("-", " ")}</option>)}</select></label>
    <div className="overflow-hidden rounded-[var(--r-lg)] border border-[var(--border)] bg-[var(--surface)]">
      {!operations.length && <div className="flex flex-col items-center gap-2 p-12 text-center text-[var(--text-dim)]"><Icon icon="history" size={28} /><p>No recorded operations.</p><p className="text-xs">Results appear after you run supported maintenance actions.</p></div>}
      {operations.map(operation => <article key={operation.id} className="flex flex-wrap items-start justify-between gap-3 border-b border-[var(--border)] p-4 last:border-0">
        <div className="min-w-0"><p className="text-sm font-medium">{operation.title}</p><p className="mt-1 text-xs text-[var(--text-dim)]"><time dateTime={operation.recordedAt}>{new Date(operation.recordedAt).toLocaleString()}</time> · {operation.tool.replaceAll("-", " ")}</p>{operation.detail && <p className="mt-2 text-xs text-[var(--text-dim)]">{operation.detail}</p>}</div>
        <div className="flex flex-wrap items-center gap-3 text-xs"><Badge tone={operation.outcome === "completed" ? "success" : operation.outcome === "failed" ? "danger" : "warning"}>{operation.outcome}</Badge>{operation.affectedItems !== undefined && <span>{operation.affectedItems.toLocaleString()} items</span>}{operation.bytesRecovered !== undefined && <span className="font-mono">{formatBytes(operation.bytesRecovered)} reported removed</span>}</div>
      </article>)}
    </div>
  </section>;
}
