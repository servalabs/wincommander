export interface WindowsAccountIdentity {
  name: string;
  displayName?: string;
  sid?: string;
  /** Returned by the Windows profile query, not copied from saved intent. */
  isCurrent?: boolean;
}

/** Select the current Windows account from a live profile/service response. */
export function getCurrentWindowsAccount<T extends WindowsAccountIdentity>(
  accounts: readonly T[],
  currentUser?: string | null,
): T | null {
  return accounts.find((account) => account.isCurrent === true)
    ?? accounts.find((account) => account.name === currentUser)
    ?? null;
}
