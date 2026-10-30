import type { CleanupCategory } from "./cleanupCategories";

export const DEFAULT_BULK_CLEAR_EXCLUDES = [
  'wlanProfiles', 'browserFootprints', 'notepadState', 'wslData',
  'dockerDesktopData', 'virtualMachineArtifacts', 'developerCaches',
  'credentialManager', 'sshState', 'passwordManagerCaches',
];

// These cards did not expose a scheduler in the original Cleanup UI. Their
// newly supported schedules require an individual choice and must never be
// included in the bulk Auto-set shortcut. Keep the matching PowerShell list
// in auto-erase.ps1 in sync.
export const MANUAL_ONLY_SCHEDULE_IDS = [
  'connectivityHistory', 'shadowCopies', 'amcache', 'searchIndex', 'srumData', 'recallDb',
  'dockerDesktopData', 'virtualMachineArtifacts', 'developerCaches', 'networkWizardHistory', 'werHistory', 'stickyNotes', 'oneDriveMetadata',
  'spotlightCache', 'fontCache', 'legacyIconCache', 'gameCaptures', 'photosCache', 'xboxCache',
  'communicationCaches', 'editorHistory', 'sshState', 'remoteAccessLogs', 'passwordManagerCaches',
  'gameLauncherLogs', 'adobeRecent', 'officeTempFiles', 'neighborCache', 'netbiosCache',
  'geolocationCache', 'vpnPhonebooks', 'proxyCache', 'bitsQueue',
] as const;

export const isManualOnlyCleanupSchedule = (id: string): boolean =>
  MANUAL_ONLY_SCHEDULE_IDS.some(manual => manual.toLowerCase() === id.toLowerCase());

export const isDefaultExcludedCleanup = (id: string): boolean =>
  DEFAULT_BULK_CLEAR_EXCLUDES.some(excluded => excluded.toLowerCase() === id.toLowerCase());

export const isAutomaticCleanupExcluded = (id: string, exclusions: ReadonlySet<string>): boolean =>
  isDefaultExcludedCleanup(id) || isManualOnlyCleanupSchedule(id) || Array.from(exclusions).some(excluded => excluded.toLowerCase() === id.toLowerCase());

const DEFAULT_WARNINGS: Record<string, string> = {
  connectivityHistory: 'This removes Windows network connection history and signatures.',
  srumData: 'This removes Windows resource-usage history; Windows may rebuild the database.',
  recallDb: 'This removes Windows activity timeline databases and their stored history.',
  wlanProfiles: 'This forgets all-user Wi-Fi profiles on available adapters, including their saved credentials. Private and policy-managed profiles are excluded. The current wireless connection may be lost.',
  browserFootprints: 'This removes browser history, cookies, saved browser passwords and other saved browser state. You may be signed out of websites.',
  notepadState: 'This can permanently delete unsaved Notepad tabs and drafts.',
  wslData: 'This can permanently delete registered Linux distributions and their files. Scheduled cleanup runs while the selected user is signed in and leaves Docker-managed distributions unchanged.',
  dockerDesktopData: 'This can permanently delete local Docker images, containers and volumes.',
  virtualMachineArtifacts: 'This can permanently delete virtual-machine snapshots and local configuration.',
  developerCaches: 'This can remove developer caches, local credentials and configuration.',
  credentialManager: 'This removes saved sign-in credentials. You may need to sign in again.',
  sshState: 'This removes loaded SSH agent identities and the saved known_hosts file, including remembered server host keys. Future SSH connections may ask you to verify servers again.',
  passwordManagerCaches: 'This removes selected 1Password, Bitwarden and LastPass cache and session files. Other password managers and custom locations are not covered. You may need to unlock or sign in again.',
};

export function getCleanupManualWarning(category: Pick<CleanupCategory, 'id' | 'confirmMessage'>): string {
  return DEFAULT_WARNINGS[category.id] ?? category.confirmMessage ?? 'This permanently removes the records shown by this card.';
}

export function getCleanupManualConfirmation(
  category: Pick<CleanupCategory, 'id' | 'label' | 'confirmMessage'>,
  action: 'clear' | 'schedule',
  minutes?: number,
  accountLabel?: string,
) {
  const warning = getCleanupManualWarning(category);
  return {
    title: action === 'schedule' ? `Schedule ${category.label}?` : `Clear ${category.label}?`,
    description: action === 'schedule'
      ? `${accountLabel ? `Windows account: ${accountLabel}. ` : ''}${warning}${category.id === 'virtualMachineArtifacts' ? ' Scheduled cleanup uses VirtualBox and VMware to remove snapshots from powered-off machines while the selected user is signed in. It also removes selected-user VMware recent history and, when providers are stopped, user-scoped VM logs. Configuration, host logs and unmanaged artifacts can remain; the run reports incomplete when they do.' : category.id === 'credentialManager' ? ' Scheduled cleanup removes Windows credentials while the selected user is signed in. Other apps’ Vault entries are not covered, so the run reports incomplete.' : category.id === 'dockerDesktopData' ? ' Scheduled cleanup targets the local Docker Desktop Linux engine while the selected user is signed in. Bind-mounted files, remote or Windows engines and external volumes are not cleared. The disk image and logs can remain; unhandled items make the run incomplete.' : ''} This card is excluded from automatic setup by default. Enabling this individual schedule authorizes this cleanup every ${minutes} minutes, including overdue work after the computer comes back online. It continues until you turn it off.`
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
    description: `Create recurring wipes for ${categories.length} cards, plus daily SSD maintenance? These wipes can repeatedly and permanently delete saved files, app data and recovery material, including overdue work after the computer comes back online. They continue after WinCommander closes until turned off.${affected} Cards reserved for individual scheduling, default exclusions and your additional exclusions will be skipped. Existing individual settings will be preserved.`,
    confirmLabel: 'Enable recurring wipes',
  };
}

export async function requestCleanupManualAuthorization(
  category: Pick<CleanupCategory, 'id' | 'label' | 'confirmMessage'>,
  action: 'clear' | 'schedule',
  confirm: (options: ReturnType<typeof getCleanupManualConfirmation>) => Promise<boolean>,
  minutes?: number,
  accountLabel?: string,
): Promise<boolean> {
  const requiresConfirmation = isDefaultExcludedCleanup(category.id) ||
    (action === 'schedule' && isManualOnlyCleanupSchedule(category.id));
  return !requiresConfirmation || await confirm(getCleanupManualConfirmation(category, action, minutes, accountLabel));
}
