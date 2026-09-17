/**
 * Fleet changes the device's managed configuration, so it belongs only in an
 * elevated administrator session. Secure Storage is deliberately separate:
 * every signed-in user can create and mount their own per-user Vaults.
 *
 * Treat an unknown elevation result as unavailable. That avoids briefly
 * exposing Fleet while the startup probe is still resolving the current
 * process token.
 */
export function canOpenFleetNavigation(isAdmin: boolean | null | undefined): boolean {
  return isAdmin === true;
}
