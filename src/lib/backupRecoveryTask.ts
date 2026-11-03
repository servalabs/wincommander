// Explicit local backup tasks survive panel navigation, never application restart.
export type BackupAction = "copied" | "checked" | "restored";
export type BackupResult = { operation: BackupAction; bytes: string; files?: number; directoryName?: string; legacy: boolean };
export type BackupTask = { state: "idle" | "running" | "done" | "failed" | "unknown"; action?: BackupAction; result?: BackupResult; destination?: string; destinationParent?: string; message?: string };
type Response = { success: boolean; data?: unknown; error?: string };

export function parseBackupResult(value: unknown, action: BackupAction): BackupResult | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const v = value as Record<string, unknown>;
  if (v.operation !== action || typeof v.bytes !== "string" || !/^\d{1,20}$/.test(v.bytes) || BigInt(v.bytes) > 18446744073709551615n || typeof v.legacy !== "boolean") return null;
  if (action === "checked") {
    if (v.directoryName !== undefined || v.legacy) return null;
  } else {
    const prefix = action === "copied" ? "WinCommander-Container-Copy" : "WinCommander-Restored-Files";
    if (typeof v.directoryName !== "string" || !new RegExp(`^${prefix}-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$`).test(v.directoryName)) return null;
    if (action === "copied" && v.legacy) return null;
  }
  if (action === "restored" && (typeof v.files !== "number" || !Number.isSafeInteger(v.files) || v.files < 0)) return null;
  if (action !== "restored" && v.files !== undefined) return null;
  return { operation: action, bytes: v.bytes, legacy: v.legacy, ...(typeof v.directoryName === "string" ? { directoryName: v.directoryName } : {}), ...(typeof v.files === "number" ? { files: v.files } : {}) };
}

export function backupFailure(error: unknown): { state: "failed" | "unknown"; message: string } {
  const text = typeof error === "string" ? error : "";
  if (/outcome_unknown|outcome unknown|session is closing|request was not replayed/i.test(text)) return { state: "unknown", message: "Outcome unknown. The operation was not replayed. Keep the destination and check its files before use." };
  if (text.includes("backup_legacy_required")) return { state: "failed", message: "This older generation has no original per-file hashes. Select legacy recovery explicitly to check and restore its current bytes." };
  if (/feature_unknown|unknown feature|unsupported_backup_operation/.test(text)) return { state: "failed", message: "Update WinCommander Pro to a version that supports encrypted backup recovery." };
  if (text.includes("backup_already_running")) return { state: "failed", message: "Another backup operation is running. Wait for it to finish before starting another." };
  if (text.includes("container unavailable, unsafe or in use")) return { state: "failed", message: "The container is unavailable, unsafe or in use. Normally dismount it before exporting." };
  if (text.includes("recovery destination must be outside the source generation")) return { state: "failed", message: "Choose a restore location outside the selected backup generation and its data folders. No restore folder was created." };
  if (text.includes("cannot verify destination ancestry")) return { state: "failed", message: "The destination's parent locations could not be checked. Choose another existing location or check parent read permissions. No restore folder was created." };
  if (text.includes("backup_not_started:")) return { state: "failed", message: "The backup worker could not start. Check that WinCommander Pro is installed and available." };
  if (text.includes("backup_rejected:")) return { state: "failed", message: "The operation did not report completion. Incomplete files may remain; retain them for inspection and use a new destination for another copy." };
  return { state: "unknown", message: "Completion could not be confirmed. The operation was not replayed. Keep the destination and check its files before use." };
}

export function createBackupTaskStore() {
  let snapshot: BackupTask = { state: "idle" };
  const listeners = new Set<() => void>();
  const update = (value: BackupTask) => { snapshot = value; for (const listener of listeners) listener(); };
  return {
    getSnapshot: () => snapshot,
    subscribe: (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; },
    async run(action: BackupAction, request: () => Promise<Response>, parent?: string) {
      if (snapshot.state === "running") return false;
      update({ state: "running", action, destinationParent: parent });
      try {
        const response = await request();
        if (!response.success) { update({ action, destinationParent: parent, ...backupFailure(response.error) }); return false; }
        const result = parseBackupResult(response.data, action);
        if (!result) { update({ state: "unknown", action, destinationParent: parent, message: "No valid completion receipt was returned. Keep the destination and verify it before use; do not assume the operation failed." }); return false; }
        const destination = parent && result.directoryName ? `${parent.replace(/[\\/]$/, "")}\\${result.directoryName}` : undefined;
        update({ state: "done", action, result, destination });
        return true;
      } catch {
        // A transport exception cannot establish whether the child completed.
        update({ action, destinationParent: parent, ...backupFailure("backup_outcome_unknown") }); return false;
      }
    },
  };
}
export const backupTask = createBackupTaskStore();
