export type CleanupScheduleScope = "system" | "current-user";

export interface CleanupScheduleRow {
  categoryId: string;
  enabled: boolean;
  state?: string;
  scope?: CleanupScheduleScope | "other-user";
  intervalMinutes: number;
  targetUser: string | null;
  managedByAutoSet?: boolean;
}

export interface CleanupScheduleRecord {
  intervalMinutes: number | null;
  managedByAutoSet: boolean;
  scope: CleanupScheduleScope;
  enabled: boolean;
}

/** Rejects ambiguous, unknown, or internally contradictory local task state. */
export function readCleanupScheduleSnapshot(rows: readonly CleanupScheduleRow[], relevantIds: ReadonlySet<string>) {
  const records: Record<string, CleanupScheduleRecord> = {};
  const schedules: Record<string, number> = {};
  const disabledSchedules: Record<string, Omit<CleanupScheduleRecord, "enabled">> = {};
  const managedIds = new Set<string>();
  for (const row of rows) {
    if (!row || typeof row.categoryId !== "string") return null;
    const id = row.categoryId === "clipboard" ? "clipboardHistory" : row.categoryId;
    if (!relevantIds.has(id) || row.targetUser || row.scope === "other-user") continue;
    if (row.scope !== "system" && row.scope !== "current-user") return null;
    if (!["Ready", "Running", "Queued", "Disabled"].includes(row.state ?? "")) return null;
    if (typeof row.managedByAutoSet !== "boolean" || typeof row.enabled !== "boolean") return null;
    const enabled = row.state !== "Disabled";
    if (row.enabled !== enabled || records[id]) return null;
    const interval = Number.isFinite(row.intervalMinutes) && row.intervalMinutes > 0 ? row.intervalMinutes : null;
    if (enabled && interval === null) return null;
    const record: CleanupScheduleRecord = { intervalMinutes: interval, managedByAutoSet: row.managedByAutoSet, scope: row.scope, enabled };
    records[id] = record;
    if (enabled && interval !== null) schedules[id] = interval;
    else disabledSchedules[id] = { intervalMinutes: interval, managedByAutoSet: row.managedByAutoSet, scope: row.scope };
    if (row.managedByAutoSet) managedIds.add(id);
  }
  return { records, schedules, disabledSchedules, managedIds };
}

export function isVerifiedManagedSchedule(record: CleanupScheduleRecord | undefined, intervalMinutes: number, scope: CleanupScheduleScope): boolean {
  return !!record && record.enabled && record.managedByAutoSet && record.intervalMinutes === intervalMinutes && record.scope === scope;
}

export function isVerifiedIndividualSchedule(record: CleanupScheduleRecord | undefined, requestedMinutes: number, returnedMinutes: number | undefined, scope: CleanupScheduleScope): boolean {
  return returnedMinutes === requestedMinutes && !!record && record.enabled && !record.managedByAutoSet && record.intervalMinutes === requestedMinutes && record.scope === scope;
}
