import type { AppSettings } from "../types/settings";
import { showError } from "../utils/toast";
import { getControlLifecycle, type ControlLifecycle } from "./settingsControlLifecycle";

export type SettingsWriteOutcome = Extract<ControlLifecycle, "Blocked" | "Failed">;

export interface SettingsWriteFailure {
  state: SettingsWriteOutcome;
  reason: string;
}

const reportedWriteFailures = new WeakSet<object>();

function errorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  return typeof error === "string" ? error : "Settings storage did not accept the change.";
}

/** Maps transport errors onto the same vocabulary used by machine controls. */
export function getSettingsWriteFailure(error: unknown): SettingsWriteFailure {
  const reason = errorText(error).trim() || "Settings storage did not accept the change.";
  const lifecycle = getControlLifecycle({ failureReason: reason });
  return { state: lifecycle.state as SettingsWriteOutcome, reason: lifecycle.reason ?? reason };
}

/** Safe to pass directly to Promise.catch at every settings-write call site. */
export function reportSettingsWriteFailure(error: unknown): void {
  if (typeof error === "object" && error !== null) {
    if (reportedWriteFailures.has(error)) return;
    reportedWriteFailures.add(error);
  }
  const failure = getSettingsWriteFailure(error);
  void showError(`Settings ${failure.state.toLowerCase()}: ${failure.reason}. Changes were reverted.`);
}

/**
 * A settings write is never optimistic in AppContext. If its IPC request
 * rejects, re-read the authoritative saved record before reporting the
 * failure. This resets controls that render from settings to their confirmed
 * value instead of leaving the UI on the requested value.
 */
export async function recoverSettingsWrite<T>(
  write: () => Promise<T>,
  restore: () => Promise<AppSettings>,
  applyRestored: (settings: AppSettings) => void,
  report: (error: unknown) => void = reportSettingsWriteFailure,
): Promise<T> {
  try {
    return await write();
  } catch (error) {
    try {
      applyRestored(await restore());
    } catch (restoreError) {
      // Preserve the last confirmed in-memory value if the settings file cannot
      // be read either. The initial write failure remains the user-facing cause.
      console.error("Failed to restore confirmed settings after a write error:", restoreError);
    }
    report(error);
    throw error;
  }
}
