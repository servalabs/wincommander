import type { MaintenanceOperation } from "../../lib/maintenanceHistory";

const TOOL_LABELS: Record<MaintenanceOperation["tool"], string> = {
  cleanup: "Cleanup",
  duplicates: "Duplicate cleanup",
  "empty-folders": "Empty folders",
  "disk-analysis": "Disk analysis",
  startup: "Startup",
  applications: "Applications",
  repair: "Repair",
};

export function maintenanceLogEvent(operation: MaintenanceOperation) {
  const severity = operation.outcome === "failed" ? "error" : operation.outcome === "completed" ? "info" : "warn";
  const safeSummary = `${TOOL_LABELS[operation.tool]} · ${operation.outcome}`;
  const safeDetail = [operation.affectedItems === undefined ? "" : `${operation.affectedItems} items`, operation.bytesRecovered === undefined ? "" : `${operation.bytesRecovered} reported bytes`].filter(Boolean).join(" · ");
  return {
    id: `maintenance:${operation.id}`,
    occurredAt: operation.recordedAt,
    source: "maintenance" as const,
    severity: severity as "error" | "warn" | "info",
    summary: operation.title,
    detail: [operation.outcome, operation.detail, safeDetail].filter(Boolean).join(" · "),
    safeSummary,
    safeDetail,
  };
}
