import { waitForSoftTimeout } from "./softTimeout";
import { getDefaultModules, getFirstRunModules, type ModuleConfig } from "../types/modules";
import type { AppSettings, ExperienceLevel } from "../types/settings";

/** Runtime read status is separate from the persisted settings schema. */
export interface PersonalSettingsStatus {
  mode: "service" | "legacy" | "temporary";
  recoveryRequired: boolean;
  canSave: boolean;
}

export function readPersonalSettingsStatus(
  settings: unknown,
  previous: PersonalSettingsStatus | null = null,
): PersonalSettingsStatus | null {
  if (!settings || typeof settings !== "object") return previous;
  const status = (settings as { personalSettingsStatus?: unknown }).personalSettingsStatus;
  if (!status || typeof status !== "object") return previous;
  const { mode, recoveryRequired, canSave } = status as Partial<PersonalSettingsStatus>;
  if ((mode !== "service" && mode !== "legacy" && mode !== "temporary")
    || typeof recoveryRequired !== "boolean" || typeof canSave !== "boolean") return previous;
  if (previous?.mode === mode && previous.recoveryRequired === recoveryRequired && previous.canSave === canSave) {
    return previous;
  }
  return { mode, recoveryRequired, canSave };
}

export function getStartupSettingsRecoveryMessage(error: unknown): string | null {
  const message = error instanceof Error ? error.message : typeof error === "string" ? error : "";
  return message.includes("SETTINGS_KEY_UNAVAILABLE:")
    ? "Windows could not unlock this account's saved settings. Your saved data is preserved. Restore this Windows account's access to its encrypted data, then retry."
    : null;
}

export function normalizeModulesConfig(
  modules: ModuleConfig | undefined,
  level: ExperienceLevel | undefined,
  firstRunComplete: boolean | undefined,
): ModuleConfig {
  const base = firstRunComplete === true
    ? getDefaultModules(level ?? "standard")
    : getFirstRunModules();
  return { ...base, ...(modules ?? {}) };
}

/** Missing UI keys need defaults, not a shared-store write during every login. */
export function normalizeStartupSettings(settings: AppSettings): AppSettings {
  return {
    ...settings,
    app: {
      ...settings.app,
      modules: normalizeModulesConfig(
        settings.app?.modules,
        settings.app?.experienceLevel,
        settings.app?.firstRunComplete,
      ),
    },
  };
}

/** Bound the UI wait and reject late hydration without claiming native cancellation. */
export async function hydrateWithinBudget<T>(
  load: (signal: AbortSignal) => Promise<T | null>,
  timeoutMs = 8_000,
): Promise<T | null> {
  const controller = new AbortController();
  try {
    const result = await waitForSoftTimeout(load(controller.signal), timeoutMs);
    return result.status === "completed" ? result.value : null;
  } catch {
    return null;
  } finally {
    controller.abort();
  }
}
