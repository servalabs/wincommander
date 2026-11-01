import type { AppSettings, SettingsPatch } from "../types/settings";
import { getByPath } from "../types/toggles";

export interface AdoptionToggle {
  settingsPath: string;
  currentPath: string;
  capabilityKey?: unknown;
}

export function buildAdoptionPatch(settings: AppSettings, toggles: readonly AdoptionToggle[]): SettingsPatch | null {
  const patch: Record<string, unknown> = {};
  let count = 0;
  for (const toggle of toggles) {
    if (toggle.capabilityKey || !toggle.settingsPath.startsWith("ideal.")) continue;
    const intent = getByPath(settings, toggle.settingsPath);
    if (intent !== null && intent !== undefined) continue;
    if (getByPath(settings, toggle.currentPath) !== true) continue;
    const parts = toggle.settingsPath.split(".");
    if (parts.some(part => !part || ["__proto__", "constructor", "prototype"].includes(part))) continue;
    let cursor = patch;
    for (const part of parts.slice(0, -1)) {
      cursor = (cursor[part] ??= {}) as Record<string, unknown>;
    }
    cursor[parts[parts.length - 1]] = true;
    count++;
  }
  return count ? patch as SettingsPatch : null;
}

export function createAdoptionAttempts(limit = 3) {
  let accepted = "";
  let active: string | null = null;
  let attemptKey = "";
  let attempts = 0;
  return {
    begin(key: string): boolean {
      if (active !== null || key === accepted) return false;
      if (key !== attemptKey) {
        attemptKey = key;
        attempts = 0;
      }
      if (attempts >= limit) return false;
      attempts++;
      active = key;
      return true;
    },
    finish(key: string, saved: boolean): void {
      if (active !== key) return;
      if (saved) accepted = key;
      active = null;
    },
  };
}
