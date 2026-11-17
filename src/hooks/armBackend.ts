import { invoke } from "@tauri-apps/api/core";
import { logRecord } from "../lib/logger";
import { showError } from "../utils/toast";

/**
 * Invoke a backend command that arms a protection (lock, decoy backstop,
 * hotkey, capture exclusion, watcher). A failure is never swallowed: it is
 * logged and shown to the user, and the caller gets `false` so it can keep its
 * visible state truthful (revert the toggle, skip success UI).
 */
export async function armBackend(
  command: string,
  args: Record<string, unknown> | undefined,
  failureMessage: string,
): Promise<boolean> {
  try {
    await invoke(command, args);
    return true;
  } catch (error) {
    reportArmFailure(command, failureMessage, error);
    return false;
  }
}

export function reportArmFailure(command: string, failureMessage: string, error: unknown): void {
  const detail = String(error);
  void logRecord("error", `${command} failed: ${detail}`);
  showError(`${failureMessage}: ${detail}`);
}
