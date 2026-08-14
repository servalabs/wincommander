type DriverFinding = {
  instanceId: string;
  name: string;
  problemCode: number | null;
};

type VulnerableDriverFinding = {
  filename: string;
  path: string;
};

const DRIVER_IGNORE_PREFIX = "driver-health:";
const BYOVD_IGNORE_PREFIX = "driver-byovd:";

function normalizeFindingPart(value: string): string {
  return value.trim().toLowerCase();
}

/** Stable, namespaced keys keep driver dismissals separate from radar findings. */
export function driverHealthIgnoreId(driver: DriverFinding): string {
  return `${DRIVER_IGNORE_PREFIX}${normalizeFindingPart(driver.instanceId || `${driver.name}:${driver.problemCode ?? "unknown"}`)}`;
}

/** A driver path takes precedence because filenames can legitimately repeat. */
export function vulnerableDriverIgnoreId(driver: VulnerableDriverFinding): string {
  return `${BYOVD_IGNORE_PREFIX}${normalizeFindingPart(driver.path || driver.filename)}`;
}

export function isIgnoredDriverFinding(ignoredFindingIds: readonly string[], id: string): boolean {
  return ignoredFindingIds.includes(id);
}

export function ignoredDriverFindingCount(ignoredFindingIds: readonly string[]): number {
  return ignoredFindingIds.filter((id) => id.startsWith(DRIVER_IGNORE_PREFIX) || id.startsWith(BYOVD_IGNORE_PREFIX)).length;
}
