const DEFAULT_INVENTORY_MAX_AGE_MS = 30 * 60 * 1000;

/**
 * The app inventory includes installed-app metadata and icon payloads, so it is
 * intentionally expensive. A new desktop window should reuse a recent cached
 * snapshot instead of immediately starting the same scan again.
 */
export function isAppInventoryRefreshDue(
  lastScanAt: string | undefined,
  nowMs: number = Date.now(),
  maxAgeMs: number = DEFAULT_INVENTORY_MAX_AGE_MS,
): boolean {
  if (!lastScanAt) return true;

  const scannedAtMs = Date.parse(lastScanAt);
  if (!Number.isFinite(scannedAtMs) || scannedAtMs > nowMs) return true;

  return nowMs - scannedAtMs >= maxAgeMs;
}
