import type { AppSettings } from "../types/settings";
import { getByPath, type ToggleDef } from "../types/toggles";
import { getToggleDrift, isToggleCheckedValue } from "./toggleDrift";

export const HEAL_COOLDOWN_MS = 60_000;
export const TOGGLE_REPAIR_VERIFIED_EVENT = "wc-toggle-repair-verified";
export interface VerifiedToggleRepair { toggleId: string; targetChecked: boolean }

export function isAutoHealEnabled(settings: AppSettings | undefined): boolean {
  return !!settings?.app.firstRunComplete && (settings.app.autoHeal === true ||
    (settings.policy?.syncMode === "managed" && !!settings.policy.lockedPaths?.length));
}

export function mayAutoHeal(settings: AppSettings, toggle: ToggleDef): boolean {
  if (!isAutoHealEnabled(settings) || toggle.isAction || toggle.irreversible ||
      (!toggle.capabilityKey && (!toggle.enableCmd || !toggle.disableCmd))) return false;
  const path = toggle.settingsPath.replace(/^ideal\./, "");
  return settings.app.autoHeal === true || (settings.policy?.syncMode === "managed" &&
    !!settings.policy.lockedPaths?.some(locked => path === locked || path.startsWith(`${locked}.`)));
}

interface HealDependencies {
  getSettings: () => AppSettings | undefined;
  toggles: readonly ToggleDef[];
  canUse: (toggle: ToggleDef) => boolean;
  probe: () => Promise<AppSettings | undefined>;
  repair: (toggle: ToggleDef, target: boolean) => Promise<void>;
  failureMessage: (error: unknown) => string;
  notify: (label: string, message: string) => void;
  verified: (repair: VerifiedToggleRepair) => void;
  now?: () => number;
}

/** Reassess fresh observations even when Windows reports the same failed state. */
export function createAutoHealer(deps: HealDependencies) {
  const attemptedAt = new Map<string, number>();
  const reportedFailures = new Map<string, string>();
  let running = false;
  const report = (id: string, label: string, error: unknown) => {
    const message = deps.failureMessage(error);
    if (reportedFailures.get(id) === message) return;
    reportedFailures.set(id, message);
    deps.notify(label, message);
  };
  return async () => {
    if (running || !isAutoHealEnabled(deps.getSettings())) return;
    running = true;
    try {
      const observed = await deps.probe();
      if (!observed || !isAutoHealEnabled(deps.getSettings())) return;
      reportedFailures.delete("probe");
      const applied: { toggle: ToggleDef; target: boolean }[] = [];
      for (const toggle of deps.toggles) {
        const latest = deps.getSettings();
        if (!latest || !mayAutoHeal(latest, toggle) || !deps.canUse(toggle)) continue;
        const drift = getToggleDrift({ ...latest, current: observed.current }, toggle);
        if (!drift) {
          if (getByPath(observed, toggle.currentPath) != null) reportedFailures.delete(toggle.id);
          continue;
        }
        if (toggle.requiresPinOnEnable && drift.targetChecked) continue;
        const now = (deps.now ?? Date.now)();
        if (now - (attemptedAt.get(toggle.id) ?? -Infinity) < HEAL_COOLDOWN_MS) continue;
        attemptedAt.set(toggle.id, now);
        try {
          await deps.repair(toggle, drift.targetChecked);
          applied.push({ toggle, target: drift.targetChecked });
        } catch (error) {
          if (isAutoHealEnabled(deps.getSettings())) report(toggle.id, toggle.label, error);
        }
      }
      if (!applied.length || !isAutoHealEnabled(deps.getSettings())) return;
      const readback = await deps.probe();
      if (!isAutoHealEnabled(deps.getSettings())) return;
      for (const { toggle, target } of applied) {
        const value = readback && getByPath(readback, toggle.currentPath);
        if (value != null && isToggleCheckedValue(value, toggle.checkedWhen) === target) {
          reportedFailures.delete(toggle.id);
          deps.verified({ toggleId: toggle.id, targetChecked: target });
        } else {
          report(toggle.id, toggle.label, "Windows has not confirmed the automatic repair. Auto Heal will check again.");
        }
      }
    } catch (error) {
      if (isAutoHealEnabled(deps.getSettings())) report("probe", "Auto Heal", error);
    } finally {
      running = false;
    }
  };
}
