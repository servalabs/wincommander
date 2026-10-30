export type CleanupScheduleScope = "system" | "current-user";

export interface CleanupScheduledOutcome {
  attemptedUtc: string;
  success: boolean;
  removed: number;
  deferred: number;
  failed: number;
}

export interface CleanupScheduleRow {
  categoryId: string;
  enabled: boolean;
  state?: string;
  scope?: CleanupScheduleScope | "other-user";
  intervalMinutes: number;
  targetUser: string | null;
  managedByAutoSet?: boolean;
  legacy?: boolean;
  lastRun?: string | null;
  lastResult?: number | null;
  nextRun?: string | null;
  cleanupOutcome?: CleanupScheduledOutcome | null;
  payloadVerified?: boolean;
}

export interface CleanupScheduleRecord {
  intervalMinutes: number | null;
  managedByAutoSet: boolean;
  scope: CleanupScheduleScope;
  enabled: boolean;
  lastRun?: string | null;
  lastResult?: number | null;
  nextRun?: string | null;
  cleanupOutcome?: CleanupScheduledOutcome | null;
  payloadVerified: boolean | null;
}

function readOutcome(value: unknown, lastRun: string | null): CleanupScheduledOutcome | null {
  if (!value || typeof value !== "object") return null;
  const row = value as Partial<CleanupScheduledOutcome>;
  if (typeof row.attemptedUtc !== "string" || !Number.isFinite(Date.parse(row.attemptedUtc)) ||
      typeof row.success !== "boolean" || ![row.removed, row.deferred, row.failed].every(n => Number.isSafeInteger(n) && (n as number) >= 0)) return null;
  const removed = row.removed as number, deferred = row.deferred as number, failed = row.failed as number;
  if (row.success !== (deferred === 0 && failed === 0)) return null;
  // The backend binds the receipt to the current generated task payload.
  if (!lastRun) return null;
  return { attemptedUtc: row.attemptedUtc, success: row.success, removed, deferred, failed };
}

/** Rejects ambiguous, unknown, or internally contradictory local task state. */
export function readCleanupScheduleSnapshot(rows: readonly CleanupScheduleRow[], relevantIds: ReadonlySet<string>) {
  const records: Record<string, CleanupScheduleRecord> = {};
  const schedules: Record<string, number> = {};
  const disabledSchedules: Record<string, Omit<CleanupScheduleRecord, "enabled">> = {};
  const managedIds = new Set<string>();
  const localRows: Record<string, Array<{ row: CleanupScheduleRow; record: CleanupScheduleRecord }>> = {};
  for (const row of rows) {
    if (!row || typeof row.categoryId !== "string") return null;
    const id = row.categoryId === "clipboard" ? "clipboardHistory" : row.categoryId;
    if (!relevantIds.has(id) || row.targetUser || row.scope === "other-user") continue;
    if (row.scope !== "system" && row.scope !== "current-user") return null;
    if (!["Ready", "Running", "Queued", "Disabled"].includes(row.state ?? "")) return null;
    if (typeof row.managedByAutoSet !== "boolean" || typeof row.enabled !== "boolean") return null;
    const enabled = row.state !== "Disabled";
    if (row.enabled !== enabled) return null;
    const interval = Number.isFinite(row.intervalMinutes) && row.intervalMinutes > 0 ? row.intervalMinutes : null;
    if (enabled && interval === null) return null;
    const lastRun = typeof row.lastRun === "string" && Number.isFinite(Date.parse(row.lastRun)) ? row.lastRun : null;
    const record: CleanupScheduleRecord = { intervalMinutes: interval, managedByAutoSet: row.managedByAutoSet, scope: row.scope, enabled, payloadVerified: typeof row.payloadVerified === "boolean" ? row.payloadVerified : null, lastRun, lastResult: typeof row.lastResult === "number" && Number.isInteger(row.lastResult) ? row.lastResult : null, nextRun: typeof row.nextRun === "string" ? row.nextRun : null, cleanupOutcome: readOutcome(row.cleanupOutcome, lastRun) };
    (localRows[id] ??= []).push({ row, record });
  }
  for (const [id, candidates] of Object.entries(localRows)) {
    let selected = candidates[0];
    if (candidates.length > 1) {
      const active = candidates.filter(({ record }) => record.enabled);
      if (active.length !== 1 || active[0].row.legacy !== false || candidates.some(({ row, record }) =>
        record.scope !== active[0].record.scope || (!record.enabled && row.legacy !== true))) return null;
      selected = active[0];
    }
    const { record } = selected;
    const { enabled, intervalMinutes: interval } = record;
    records[id] = record;
    if (enabled && interval !== null) schedules[id] = interval;
    else disabledSchedules[id] = { intervalMinutes: interval, managedByAutoSet: record.managedByAutoSet, scope: record.scope, payloadVerified: record.payloadVerified };
    if (record.managedByAutoSet) managedIds.add(id);
  }
  return { records, schedules, disabledSchedules, managedIds };
}

export function isVerifiedManagedSchedule(record: CleanupScheduleRecord | undefined, intervalMinutes: number, scope: CleanupScheduleScope): boolean {
  return !!record && record.enabled && record.payloadVerified === true && record.managedByAutoSet && record.intervalMinutes === intervalMinutes && record.scope === scope;
}

export function isVerifiedIndividualSchedule(record: CleanupScheduleRecord | undefined, requestedMinutes: number, returnedMinutes: number | undefined, scope: CleanupScheduleScope): boolean {
  return returnedMinutes === requestedMinutes && !!record && record.enabled && record.payloadVerified === true && !record.managedByAutoSet && record.intervalMinutes === requestedMinutes && record.scope === scope;
}

/** The local snapshot cannot describe a different Windows account. */
export function canShowCleanupScheduleForSelection(isCurrentUser: boolean, isSystemWide: boolean, record?: CleanupScheduleRecord): boolean {
  return isCurrentUser || (isSystemWide && record?.scope === "system");
}
