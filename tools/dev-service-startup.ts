export interface DevelopmentVaultState { state: "free" | "mounted" | "unknown"; mountedCount: number }

export function readDevelopmentVaultState(output: string, exitCode: number | null): DevelopmentVaultState {
  const unknown: DevelopmentVaultState = { state: "unknown", mountedCount: 0 };
  if (exitCode !== 0 || output.length > 4096) return unknown;
  try {
    const value = JSON.parse(output.trim()) as DevelopmentVaultState;
    if (!value || !["free", "mounted", "unknown"].includes(value.state)
      || !Number.isSafeInteger(value.mountedCount) || value.mountedCount < 0 || value.mountedCount > 26
      || (value.state === "mounted" && value.mountedCount === 0)
      || (value.state !== "mounted" && value.mountedCount !== 0)) return unknown;
    return { state: value.state, mountedCount: value.mountedCount };
  } catch { return unknown; }
}

// The authenticated settings service is required in Free desktop development too.
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
