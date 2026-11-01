// Retain observations returned with a failed read-only scan. The failure flag
// remains authoritative and prevents a partial result from enabling cleanup.
export function cleanupScanFailure<T>(command: string, result: unknown, message: string, retainScanData: boolean): { success: false; error: string; data?: T } {
  const data = retainScanData && command.startsWith("Get-") && result !== null && typeof result === "object" && !Array.isArray(result)
    ? result as T : undefined;
  return data === undefined ? { success: false, error: message } : { success: false, error: message, data };
}
