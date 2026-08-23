import type { StartupJobId } from "../events/startup";

export interface StartupEligibility {
  hasVerifiedPaidEntitlement: boolean;
  isProInstalled: boolean;
  isProConfigured: boolean;
  autoUpdateEnabled: boolean;
  meshEnabled: boolean;
  dependenciesEnabled: boolean;
  hasIdleWindow: boolean;
}

const ALWAYS_SAFE_JOBS = new Set<StartupJobId>([
  "settings-cache",
  "system-probe",
  "startup-status",
  "panel-preload",
  "disk-cleanup-preload",
  "search-preload",
]);

/**
 * Gate optional launch work before scheduling it. Unknown entitlement/config
 * states resolve to false because a paid or convenience supervisor is never a
 * prerequisite for getting the dashboard on screen.
 */
export function canRunStartupJob(
  job: StartupJobId,
  eligibility: StartupEligibility,
): boolean {
  if (ALWAYS_SAFE_JOBS.has(job)) return true;

  switch (job) {
    case "pro-install-status":
    case "pro-manifest":
      return (
        eligibility.hasVerifiedPaidEntitlement && eligibility.isProConfigured
      );
    case "pro-hash":
    case "defender-status":
      return (
        eligibility.hasVerifiedPaidEntitlement &&
        eligibility.isProConfigured &&
        eligibility.isProInstalled &&
        eligibility.autoUpdateEnabled
      );
    case "dependencies":
      return eligibility.dependenciesEnabled;
    case "mesh-status":
      return eligibility.hasVerifiedPaidEntitlement && eligibility.meshEnabled;
    case "app-inventory":
      return eligibility.hasIdleWindow;
  }

  return false;
}
