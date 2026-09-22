/**
 * Resolve a Fleet config-epoch lock against an ideal-relative setting path.
 *
 * The device receives ideal-relative paths (for example
 * `privacy.screenCapture.reportToFleet`), while a few legacy epochs include
 * the `ideal.` prefix.  Treat an ancestor as locked too: locking
 * `privacy.screenCapture` must also lock its report switch.
 */
export function isFleetReportingPathLocked(
  lockedPaths: readonly string[] | null | undefined,
  idealPath: string,
): boolean {
  return (lockedPaths ?? []).some((rawPath) => {
    const path = rawPath.trim().replace(/^ideal\./, "");
    return path.length > 0 && (path === idealPath || idealPath.startsWith(`${path}.`));
  });
}

/** A Fleet-wide reporting requirement locks each individual local reporter
 * only when that signed master path is present too. This avoids presenting a
 * stale or locally-created value as administrator-enforced. */
export function isFleetReportControlLocked({
  lockedPaths,
  reportPath,
  requireAllDeviceAlertsInFleet,
}: {
  lockedPaths: readonly string[] | null | undefined;
  reportPath: string;
  requireAllDeviceAlertsInFleet: boolean;
}): boolean {
  return isFleetReportingPathLocked(lockedPaths, reportPath)
    || (requireAllDeviceAlertsInFleet
      && isFleetReportingPathLocked(lockedPaths, "security.requireAllDeviceAlertsInFleet"));
}
