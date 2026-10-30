import type { BcuApplication } from "../../hooks/useBackend";

export function programRemovalUnavailable(app: BcuApplication, apps: BcuApplication[]) {
  if (app.isProtected || app.isSystemComponent) return "Protected Windows or application component";
  if (app.displayName.includes(",")) return "This program's name cannot be targeted safely by the current uninstall engine";
  if (apps.filter(other => other.displayName.toLowerCase() === app.displayName.toLowerCase()).length > 1) return "Multiple programs share this name; use Windows Installed apps to choose the exact entry";
  return undefined;
}
