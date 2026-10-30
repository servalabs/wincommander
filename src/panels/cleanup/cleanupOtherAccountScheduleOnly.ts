import type { CleanupCategory } from './cleanupCategories';

// These categories support per-account schedules but do not have a supported
// other-account scan and clear path. Their cards must never reuse current-user
// results or offer those actions while another account is selected.
export const OTHER_ACCOUNT_SCHEDULE_ONLY_IDS = new Set([
  'wslData', 'dockerDesktopData', 'virtualMachineArtifacts',
  'credentialManager', 'sshState', 'passwordManagerCaches',
]);

export function isOtherAccountScheduleOnly(category: CleanupCategory, selectedOwnerSid: string | null, isViewingAllUsers: boolean): boolean {
  return !!selectedOwnerSid && !isViewingAllUsers && !!category.scopeAware && !!category.schedulable &&
    !category.schedulerRunAsSystem && OTHER_ACCOUNT_SCHEDULE_ONLY_IDS.has(category.id);
}
