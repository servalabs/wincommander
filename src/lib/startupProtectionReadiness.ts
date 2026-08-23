export type StartupProtectionOutcome = "protection_not_required" | "protection_required_ready" | "protection_failed";

export const STARTUP_PROTECTION_OPERATIONS = [
  "decoy-monitor",
  "ransomware-monitor",
  "remote-access-monitor",
  "usb-security",
  "wifi-guard",
  "auth-anomaly-monitor",
  "screen-capture-watch",
] as const;

export type StartupProtectionOperation = (typeof STARTUP_PROTECTION_OPERATIONS)[number];

export interface StartupProtectionReadiness {
  configure(ids: readonly StartupProtectionOperation[]): StartupProtectionOutcome | null;
  report(id: StartupProtectionOperation, succeeded: boolean): StartupProtectionOutcome | null;
}

/** Aggregates only actual rearm/reconcile outcomes; cache hydration is excluded. */
export function createStartupProtectionReadiness(
  emit: (outcome: StartupProtectionOutcome) => void,
): StartupProtectionReadiness {
  const pending = new Set<StartupProtectionOperation>();
  let configured = false;
  let emitted: StartupProtectionOutcome | null = null;
  const publish = (outcome: StartupProtectionOutcome) => {
    if (emitted) return null;
    emitted = outcome;
    emit(outcome);
    return outcome;
  };

  return {
    configure(ids) {
      if (configured || emitted) return null;
      configured = true;
      ids.forEach((id) => pending.add(id));
      return pending.size === 0 ? publish("protection_not_required") : null;
    },
    report(id, succeeded) {
      if (!configured || emitted || !pending.has(id)) return null;
      if (!succeeded) return publish("protection_failed");
      pending.delete(id);
      return pending.size === 0 ? publish("protection_required_ready") : null;
    },
  };
}
