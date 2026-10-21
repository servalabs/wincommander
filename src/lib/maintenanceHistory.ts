export const MAINTENANCE_HISTORY_LIMIT = 100;
export const MAINTENANCE_HISTORY_KEY = "wincommander.maintenance-history.v1";

export const MAINTENANCE_TOOLS = ["cleanup", "duplicates", "empty-folders", "disk-analysis", "startup", "applications", "repair"] as const;
export const MAINTENANCE_OUTCOMES = ["completed", "partial", "failed", "cancelled"] as const;

export interface MaintenanceOperationInput {
  tool: typeof MAINTENANCE_TOOLS[number];
  title: string;
  outcome: typeof MAINTENANCE_OUTCOMES[number];
  affectedItems?: number;
  bytesRecovered?: number;
  detail?: string;
}

export interface MaintenanceOperation extends MaintenanceOperationInput {
  id: string;
  recordedAt: string;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isCount(value: unknown): value is number | undefined {
  return value === undefined || (typeof value === "number" && Number.isSafeInteger(value) && value >= 0);
}

export function isMaintenanceOperation(value: unknown): value is MaintenanceOperation {
  if (!isRecord(value)) return false;
  return typeof value.id === "string" && value.id.length <= 80
    && typeof value.recordedAt === "string" && Number.isFinite(Date.parse(value.recordedAt))
    && MAINTENANCE_TOOLS.some(tool => tool === value.tool)
    && MAINTENANCE_OUTCOMES.some(outcome => outcome === value.outcome)
    && typeof value.title === "string" && value.title.length > 0 && value.title.length <= 120
    && (value.detail === undefined || (typeof value.detail === "string" && value.detail.length <= 240))
    && isCount(value.affectedItems) && isCount(value.bytesRecovered);
}

export function parseMaintenanceHistory(serialized: string | null): MaintenanceOperation[] {
  if (!serialized || serialized.length > 100_000) return [];
  try {
    const parsed: unknown = JSON.parse(serialized);
    return Array.isArray(parsed) ? parsed.filter(isMaintenanceOperation).slice(0, MAINTENANCE_HISTORY_LIMIT) : [];
  } catch {
    return [];
  }
}

export function appendMaintenanceOperation(entries: MaintenanceOperation[], entry: MaintenanceOperation): MaintenanceOperation[] {
  if (!isMaintenanceOperation(entry)) return entries;
  return [entry, ...entries.filter(previous => previous.id !== entry.id)].slice(0, MAINTENANCE_HISTORY_LIMIT);
}
