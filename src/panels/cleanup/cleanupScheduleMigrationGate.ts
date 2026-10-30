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

export function canAutoSetCleanupSchedules(schedulesEnabled: boolean, state: string, migrationIncomplete: boolean): boolean {
  return schedulesEnabled && state === "verified" && !migrationIncomplete;
}
