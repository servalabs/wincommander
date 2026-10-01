// The authenticated settings service is required in Free development too.
export async function syncDevelopmentService(
  freeOnly: boolean,
  run: (args: string[]) => Promise<number>,
): Promise<void> {
  const result = await run([
    "-NoProfile", "-ExecutionPolicy", "Bypass", "-File",
    "tools/sync-dev-service.ps1",
    ...(!freeOnly ? ["-SyncPro"] : []),
  ]);
  if (result !== 0) {
    throw new Error(`development service synchronization failed (exit ${result}); Vite was not started`);
  }
}
