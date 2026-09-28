/**
 * The persisted choice is intentionally not enough on its own. Think of it
 * like a workshop key that only fits a workshop door: a copied setting cannot
 * expose developer tools in a shipped application or an ordinary browser.
 */
export function shouldAllowDeveloperContextMenu(
  isNativeDebugBuild: boolean,
  userOptedIn: boolean | undefined,
): boolean {
  return isNativeDebugBuild && userOptedIn === true;
}
