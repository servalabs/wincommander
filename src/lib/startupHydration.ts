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

export type BudgetedHydration<T> =
  | { outcome: "ready"; value: T }
  | { outcome: "timed-out" }
  | { outcome: "failed" };

/**
 * Separates a merely late native read from an actual rejected/null result.
 * Callers need that distinction: only a timeout may continue in the
 * background; a known failure must show its recovery path immediately.
 */
export async function hydrateWithStatus<T>(
  load: (signal: AbortSignal) => Promise<T | null>,
  timeoutMs = 8_000,
): Promise<BudgetedHydration<T>> {
  const controller = new AbortController();
  try {
    const result = await waitForSoftTimeout(load(controller.signal), timeoutMs);
    if (result.status === "timed-out") return { outcome: "timed-out" };
    return result.value === null ? { outcome: "failed" } : { outcome: "ready", value: result.value };
  } catch {
    return { outcome: "failed" };
  } finally {
    controller.abort();
  }
}

/** Compatibility convenience for callers that only need a value/null. */
export async function hydrateWithinBudget<T>(
  load: (signal: AbortSignal) => Promise<T | null>,
  timeoutMs = 8_000,
): Promise<T | null> {
  const result = await hydrateWithStatus(load, timeoutMs);
  return result.outcome === "ready" ? result.value : null;
}

export type StartupHydrationContinuation<T> =
  | { outcome: "ready"; value: T }
  | { outcome: "failed" }
  | { outcome: "cancelled" };

/**
 * The initial budget protects responsiveness, but it cannot cancel a shared
 * native DPAPI/filesystem read. When that budget expires, surface a neutral
 * status and keep waiting for the same read. A true rejection/null result is
 * still reported to the caller as a failure; no default settings are invented.
 */
export async function continueStartupSettingsHydration<T>(
  load: (signal: AbortSignal) => Promise<T | null>,
  onSlow: () => boolean | void,
  timeoutMs = 8_000,
): Promise<StartupHydrationContinuation<T>> {
  const withinBudget = await hydrateWithStatus(load, timeoutMs);
  if (withinBudget.outcome === "ready") return withinBudget;
  if (withinBudget.outcome === "failed") return withinBudget;

  if (onSlow() === false) return { outcome: "cancelled" };
  const controller = new AbortController();
  try {
    const eventual = await load(controller.signal);
    return eventual === null ? { outcome: "failed" } : { outcome: "ready", value: eventual };
  } catch {
    return { outcome: "failed" };
  } finally {
    controller.abort();
  }
}
