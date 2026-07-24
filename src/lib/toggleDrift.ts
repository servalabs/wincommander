import type { AppSettings } from "../types/settings";
import { getByPath, type ToggleDef } from "../types/toggles";

export interface ToggleDrift {
  toggle: ToggleDef;
  targetChecked: boolean;
}

export function isToggleCheckedValue(value: unknown, checkedWhen?: string): boolean {
  if (checkedWhen !== undefined) return value === checkedWhen;
  return value === true;
}

export function getToggleDrift(appSettings: AppSettings, toggle: ToggleDef): ToggleDrift | null {
  if (toggle.irreversible) return null;

  const idealRaw = getByPath(appSettings, toggle.settingsPath);
  if (idealRaw === null || idealRaw === undefined) return null;

  const currentRaw = getByPath(appSettings, toggle.currentPath);
  if (currentRaw === null) return null;

  const targetChecked = isToggleCheckedValue(idealRaw, toggle.checkedWhen);
  const currentChecked = isToggleCheckedValue(currentRaw, toggle.checkedWhen);
  if (targetChecked === currentChecked) return null;

  return { toggle, targetChecked };
}
