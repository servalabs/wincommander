import type { CleanupCategory } from "./cleanupCategories";

export const DEFAULT_BULK_CLEAR_EXCLUDES = [
  'wlanProfiles', 'browserFootprints', 'notepadState', 'wslData',
  'dockerDesktopData', 'virtualMachineArtifacts', 'developerCaches',
  'credentialManager', 'sshState', 'passwordManagerCaches',
];

export const isDefaultExcludedCleanup = (id: string): boolean =>
  DEFAULT_BULK_CLEAR_EXCLUDES.some(excluded => excluded.toLowerCase() === id.toLowerCase());

export const isAutomaticCleanupExcluded = (id: string, exclusions: ReadonlySet<string>): boolean =>
  isDefaultExcludedCleanup(id) || Array.from(exclusions).some(excluded => excluded.toLowerCase() === id.toLowerCase());

const DEFAULT_WARNINGS: Record<string, string> = {
  wlanProfiles: 'This removes saved Wi-Fi networks and passwords. The current wireless connection may be lost.',
  browserFootprints: 'This removes browser history, cookies and saved browser state. You may be signed out of websites.',
  notepadState: 'This can permanently delete unsaved Notepad tabs and drafts.',
  wslData: 'This can permanently delete Linux distribution storage and local files.',
  dockerDesktopData: 'This can permanently delete local Docker images, containers and volumes.',
  virtualMachineArtifacts: 'This can permanently delete virtual-machine snapshots and local configuration.',
  developerCaches: 'This can remove developer caches, local credentials and configuration.',
  credentialManager: 'This removes saved sign-in credentials. You may need to sign in again.',
  sshState: 'This removes loaded SSH agent identities. Active SSH connections may need authentication again.',
  passwordManagerCaches: 'This removes password-manager caches and session state. You may need to unlock or sign in again.',
};

export function getCleanupManualWarning(category: Pick<CleanupCategory, 'id' | 'confirmMessage'>): string {
  return DEFAULT_WARNINGS[category.id] ?? category.confirmMessage ?? 'This permanently removes the records shown by this card.';
}

export function getCleanupManualConfirmation(
  category: Pick<CleanupCategory, 'id' | 'label' | 'confirmMessage'>,
  action: 'clear' | 'schedule',
  minutes?: number,
) {
  const warning = getCleanupManualWarning(category);
  return {
    title: action === 'schedule' ? `Schedule ${category.label}?` : `Clear ${category.label}?`,
    description: action === 'schedule'
      ? `${warning} This card is excluded from automatic setup by default. Enabling this individual schedule authorizes the same wipe every ${minutes} minutes, including overdue work after the computer comes back online. It continues until you turn it off.`
      : `${warning} This card is excluded from bulk cleanup by default. Continue with this individual clear? This cannot be undone.`,
    confirmLabel: action === 'schedule' ? 'Enable recurring wipe' : 'Clear this card',
  };
}

export function isCleanupCommandSuccessful(response: { success?: boolean; data?: unknown }): boolean {
  const receipt = response.data as { ok?: boolean; error?: boolean; partial?: boolean; complete?: boolean; status?: string; stdout?: string } | undefined;
  return response.success === true && receipt?.ok !== false && receipt?.error !== true && receipt?.partial !== true && receipt?.complete !== false && !['partial', 'failed', 'error', 'blocked'].includes(receipt?.status ?? '') && !/^fail:/im.test(receipt?.stdout ?? '');
}

export function getCleanupAutoScheduleConfirmation(categories: readonly CleanupCategory[]) {
  const savedData = categories.filter(category => category.usabilityTier === 'data-accounts-recovery' || category.id === 'shadowCopies');
  const affected = savedData.length ? ` Cards that can remove saved data or recovery material: ${savedData.map(category => category.label).join(', ')}.` : '';
  return {
    title: 'Enable recurring cleanup?',
    description: `Create recurring wipes for ${categories.length} cards, plus daily SSD maintenance? These wipes can repeatedly and permanently delete saved files, app data and recovery material, including overdue work after the computer comes back online. They continue after WinCommander closes until turned off.${affected} Default exclusions and your additional exclusions will be skipped. Existing individual settings will be preserved.`,
    confirmLabel: 'Enable recurring wipes',
  };
}

export async function requestCleanupManualAuthorization(
  category: Pick<CleanupCategory, 'id' | 'label' | 'confirmMessage'>,
  action: 'clear' | 'schedule',
  confirm: (options: ReturnType<typeof getCleanupManualConfirmation>) => Promise<boolean>,
  minutes?: number,
): Promise<boolean> {
  return !isDefaultExcludedCleanup(category.id) || await confirm(getCleanupManualConfirmation(category, action, minutes));
}
