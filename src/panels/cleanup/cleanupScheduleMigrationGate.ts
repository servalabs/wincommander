export function isVerifiedCleanupMigration(result: { success?: boolean; data?: { status?: string; migrated?: unknown; error?: boolean } }): boolean {
  return result.success === true && result.data?.status === "ok" && result.data.error !== true && Array.isArray(result.data.migrated);
}

/** A failed upgrade remains retryable; schedule readback is independent. */
export function createCleanupScheduleMigrationGate() {
  let completed = false;
  let pending: Promise<boolean> | null = null;
  return {
    isCompleted: () => completed,
    ensure: (migrate: () => Promise<boolean>): Promise<boolean> => {
      if (completed) return Promise.resolve(true);
      if (pending) return pending;
      pending = (async () => {
        try {
          const success = await Promise.resolve().then(migrate);
          if (success) completed = true;
          return success;
        } catch {
          return false;
        } finally {
          pending = null;
        }
      })();
      return pending;
    },
  };
}

export function canAutoSetCleanupSchedules(schedulesEnabled: boolean, state: string, migrationIncomplete: boolean, hasManagedSchedules = false): boolean {
  // A verified set of existing managed schedules can still be turned off
  // while an unrelated legacy task needs repair. Creating defaults waits for
  // migration to complete.
  return schedulesEnabled && state === "verified" && (!migrationIncomplete || hasManagedSchedules);
}

/** An altered task must be reviewed in Windows, not changed by Auto-set. */
export function requiresCleanupTaskRepair(record: { requiresTaskSchedulerRepair?: boolean } | undefined): boolean {
  return record?.requiresTaskSchedulerRepair === true;
}
