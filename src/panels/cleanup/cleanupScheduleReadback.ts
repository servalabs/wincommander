export type CleanupScheduleScope = "system" | "current-user" | "other-user";

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
  scope?: CleanupScheduleScope | "other-user" | "unknown";
  intervalMinutes: number;
  targetUser: string | null;
  ownerSid?: string | null;
  managedByAutoSet?: boolean;
  legacy?: boolean;
  lastRun?: string | null;
  lastResult?: number | null;
  nextRun?: string | null;
  cleanupOutcome?: CleanupScheduledOutcome | null;
  payloadVerified?: boolean;
  requiresTaskSchedulerRepair?: boolean;
}

export interface CleanupScheduleRecord {
  intervalMinutes: number | null;
  managedByAutoSet: boolean;
  scope: CleanupScheduleScope | "unknown";
  ownerSid?: string | null;
  enabled: boolean;
  lastRun?: string | null;
  lastResult?: number | null;
  nextRun?: string | null;
  cleanupOutcome?: CleanupScheduledOutcome | null;
  payloadVerified: boolean | null;
  requiresTaskSchedulerRepair?: boolean;
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

const accountSid = (value: unknown): value is string => typeof value === "string" && /^S-1-5-21-(?:\d+-){3}\d+$/i.test(value);

/** Rejects ambiguous or contradictory state and never attributes another account's task by name. */
export function readCleanupScheduleSnapshot(rows: readonly CleanupScheduleRow[], relevantIds: ReadonlySet<string>, selection?: { ownerSid: string }) {
  if (selection && !accountSid(selection.ownerSid)) return null;
  const records: Record<string, CleanupScheduleRecord> = {};
  const schedules: Record<string, number> = {};
  const disabledSchedules: Record<string, Omit<CleanupScheduleRecord, "enabled" | "scope"> & { scope: CleanupScheduleScope }> = {};
  const managedIds = new Set<string>();
  const localRows: Record<string, Array<{ row: CleanupScheduleRow; record: CleanupScheduleRecord }>> = {};
  const unknownRows: Record<string, CleanupScheduleRecord> = {};
  for (const row of rows) {
    if (!row || typeof row.categoryId !== "string") return null;
    const id = row.categoryId === "clipboard" ? "clipboardHistory" : row.categoryId;
    if (!relevantIds.has(id)) continue;
    if (selection) {
      if (row.scope !== "other-user" && row.scope !== "unknown") continue;
      if (row.scope === "other-user") {
        // An older or malformed readback cannot prove which account owns a task.
        if (!accountSid(row.ownerSid)) return null;
        if (row.ownerSid.toLowerCase() !== selection.ownerSid.toLowerCase()) continue;
        if (typeof row.targetUser !== "string" || !row.targetUser.trim()) return null;
      }
    } else if (row.scope === "other-user" || (row.targetUser && row.scope !== "unknown")) continue;
    if (row.scope === "unknown") {
      // An attributable task with an unresolved owner blocks only this category.
      // Never assign it to the current user or allow mutation on that basis.
      if (row.targetUser !== null || row.payloadVerified !== false || row.requiresTaskSchedulerRepair !== true ||
          typeof row.enabled !== "boolean" || typeof row.managedByAutoSet !== "boolean" ||
          !["Ready", "Running", "Queued", "Disabled"].includes(row.state ?? "") || row.enabled !== (row.state !== "Disabled")) return null;
      unknownRows[id] = { intervalMinutes: Number.isFinite(row.intervalMinutes) && row.intervalMinutes > 0 ? row.intervalMinutes : null,
        managedByAutoSet: false, scope: "unknown", enabled: row.enabled, payloadVerified: false, requiresTaskSchedulerRepair: true };
      continue;
    }
    if (row.scope !== "system" && row.scope !== "current-user" && !(selection && row.scope === "other-user")) return null;
    if (!["Ready", "Running", "Queued", "Disabled"].includes(row.state ?? "")) return null;
    if (typeof row.managedByAutoSet !== "boolean" || typeof row.enabled !== "boolean") return null;
    const enabled = row.state !== "Disabled";
    if (row.enabled !== enabled) return null;
    const interval = Number.isFinite(row.intervalMinutes) && row.intervalMinutes > 0 ? row.intervalMinutes : null;
    // Keep a known task manageable even if its repetition cannot be decoded.
    // An absent cadence cannot verify a runnable payload.
    if (enabled && interval === null && row.payloadVerified !== false) return null;
    const lastRun = typeof row.lastRun === "string" && Number.isFinite(Date.parse(row.lastRun)) ? row.lastRun : null;
    const record: CleanupScheduleRecord = { intervalMinutes: interval, managedByAutoSet: row.managedByAutoSet, scope: row.scope, ownerSid: row.scope === "other-user" ? row.ownerSid : undefined, enabled, payloadVerified: typeof row.payloadVerified === "boolean" ? row.payloadVerified : null, requiresTaskSchedulerRepair: row.requiresTaskSchedulerRepair === true, lastRun, lastResult: typeof row.lastResult === "number" && Number.isInteger(row.lastResult) ? row.lastResult : null, nextRun: typeof row.nextRun === "string" ? row.nextRun : null, cleanupOutcome: row.scope === "other-user" ? null : readOutcome(row.cleanupOutcome, lastRun) };
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
    // A disabled legacy task may be visible alongside the canonical task.
    // Do not hide a task that needs Windows review when choosing the active row.
    const record = unknownRows[id] || candidates.some(candidate => candidate.record.requiresTaskSchedulerRepair)
      ? { ...selected.record, requiresTaskSchedulerRepair: true }
      : selected.record;
    const { enabled, intervalMinutes: interval } = record;
    records[id] = record;
    if (enabled && interval !== null) schedules[id] = interval;
    else if (!enabled) disabledSchedules[id] = { intervalMinutes: interval, managedByAutoSet: record.managedByAutoSet, scope: record.scope as CleanupScheduleScope, payloadVerified: record.payloadVerified, requiresTaskSchedulerRepair: record.requiresTaskSchedulerRepair };
    if (record.managedByAutoSet) managedIds.add(id);
  }
  for (const [id, record] of Object.entries(unknownRows)) {
    if (!records[id]) records[id] = record;
  }
  return { records, schedules, disabledSchedules, managedIds };
}

export function isVerifiedManagedSchedule(record: CleanupScheduleRecord | undefined, intervalMinutes: number, scope: CleanupScheduleScope): boolean {
  return !!record && record.enabled && !record.requiresTaskSchedulerRepair && record.payloadVerified === true && record.managedByAutoSet && record.intervalMinutes === intervalMinutes && record.scope === scope;
}

export function isVerifiedIndividualSchedule(record: CleanupScheduleRecord | undefined, requestedMinutes: number, returnedMinutes: number | undefined, scope: CleanupScheduleScope, ownerSid?: string): boolean {
  return (scope !== "other-user" || (accountSid(ownerSid) && accountSid(record?.ownerSid) && ownerSid.toLowerCase() === record.ownerSid.toLowerCase())) && returnedMinutes === requestedMinutes && !!record && record.enabled && !record.requiresTaskSchedulerRepair && record.payloadVerified === true && !record.managedByAutoSet && record.intervalMinutes === requestedMinutes && record.scope === scope;
}

/** The local snapshot cannot describe a different Windows account. */
export function canShowCleanupScheduleForSelection(isCurrentUser: boolean, isSystemWide: boolean, record?: CleanupScheduleRecord): boolean {
  return isCurrentUser || (isSystemWide && record?.scope === "system");
}
