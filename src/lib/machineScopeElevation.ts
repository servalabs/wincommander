/** Plain-language fallback shown when Windows has not granted elevation. */
export const MACHINE_SCOPE_ELEVATION_MESSAGE = "Blocked: needs-elevation. Requires an administrator.";

/**
 * Machine-scope and otherwise privileged controls must not dispatch until the
 * current Windows account is confirmed as an administrator. Unknown is denied
 * so startup cache gaps cannot turn into an unguarded write.
 */
export function isPrivilegedWriteBlocked(needsAdmin: boolean, isAdmin: boolean | null | undefined): boolean {
  return needsAdmin && isAdmin !== true;
}
