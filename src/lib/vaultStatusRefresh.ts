import { vaultOperationError } from "./vaultOperationFeedback";

/** Read-only observation coordinator; never substitutes stale data for a failed read. */
export function createVaultStatusRefresh<T>(
  publish: (value: T | null) => void,
  setRefreshing: (value: boolean) => void = () => {},
  onError: (error: unknown | null) => void = () => {},
  setForegroundRefreshing: (value: boolean) => void = () => {},
) {
  let latestRequest = 0;
  let pending = 0;
  let foregroundPending = 0;
  let active: Promise<T | null> | null = null;
  const refresh = (read: () => Promise<T | null>, background = false): Promise<T | null> => {
    // Timer ticks may join ongoing verification; manual/post-mutation reads
    // always start a fresh probe and supersede older background observations.
    if (background && active) return active;
    const request = ++latestRequest;
    if (++pending === 1) setRefreshing(true);
    if (!background && ++foregroundPending === 1) setForegroundRefreshing(true);
    const operation = (async () => {
      try {
        const value = await Promise.resolve().then(read);
        if (request !== latestRequest) return null;
        if (value !== null) { publish(value); onError(null); }
        else onError(new Error("vault_inventory_unavailable"));
        return value;
      } catch (error) {
        if (request === latestRequest) onError(error);
        return null;
      } finally {
        if (--pending === 0) setRefreshing(false);
        if (!background && --foregroundPending === 0) setForegroundRefreshing(false);
        if (request === latestRequest) active = null;
      }
    })();
    active = operation;
    return operation;
  };
  return Object.assign(refresh, { reset: () => { ++latestRequest; active = null; publish(null); onError(null); } });
}

export function vaultInventoryFailureMessage(error: unknown): string {
  const detail = error instanceof Error ? error.message : typeof error === "string" ? error : "";
  if (detail.includes("vault_runtime_update_required") || detail.includes("vault_service_personal_status_invalid")) return vaultOperationError(detail);
  if (detail.includes("caller_root_unavailable")) return "Windows could not verify this account's mounted drive. This does not prove that the encrypted volume was dismounted. Refresh status before using it.";
  return "Mounted-volume status could not be refreshed. Any listed drives are from the last confirmed check and may have changed. Refresh status before using them.";
}
