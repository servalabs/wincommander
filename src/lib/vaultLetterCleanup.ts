// SPDX-License-Identifier: AGPL-3.0-or-later
const CATEGORIES = ["real_windows_volume", "active_wincommander_mount", "stale_wincommander_reservation", "duplicate_saved_entry", "system_or_policy_reserved_letter", "external_or_unknown_owner"] as const;
export interface VaultLetterReport { letter: string; category: typeof CATEGORIES[number]; released: boolean; saved_entry: boolean }
export interface VaultLetterCleanup { released: number; letters: VaultLetterReport[] }

export function readVaultLetterCleanup(value: unknown): VaultLetterCleanup {
  const result = value as VaultLetterCleanup | null;
  const invalid = () => { throw new Error("vault_drive_letter_cleanup_unverified"); };
  if (!result || !Number.isSafeInteger(result.released) || result.released < 0 || result.released > 26
    || !Array.isArray(result.letters) || result.letters.length > 26) return invalid();
  const seen = new Set<string>();
  for (const report of result.letters) {
    if (!report || typeof report.letter !== "string" || !/^[A-Z]$/.test(report.letter) || seen.has(report.letter) || !CATEGORIES.includes(report.category)
      || typeof report.released !== "boolean" || typeof report.saved_entry !== "boolean"
      || (report.released && (report.letter <= "C" || report.category !== "stale_wincommander_reservation" || report.saved_entry))) return invalid();
    seen.add(report.letter);
  }
  if (result.letters.filter(report => report.released).length !== result.released) return invalid();
  return result;
}

export function vaultLetterReportMessage(report: VaultLetterReport): string {
  const prefix = `${report.letter}: `;
  if (report.letter <= "C") return `${prefix}is protected by Windows and cannot be released here.`;
  if (report.released) return `${prefix}a confirmed stale WinCommander reservation was released.`;
  switch (report.category) {
    case "real_windows_volume": return `${prefix}is in use by a mounted Windows volume and cannot be released by WinCommander.`;
    case "active_wincommander_mount": return `${prefix}is held by an active Vault or recovery operation. Check Secure Storage; finish the operation or use its authorized recovery action.`;
    case "duplicate_saved_entry": return `${prefix}is selected by duplicate saved entries. Edit or remove the duplicate shortcuts; no Windows mapping was changed.`;
    case "system_or_policy_reserved_letter": return report.saved_entry
      ? `${prefix}is reserved by a saved Quick Mount entry. Remove or edit that entry to release its preference.`
      : `${prefix}is protected by Windows or Vault policy and cannot be released here.`;
    case "stale_wincommander_reservation": return `${prefix}has a WinCommander recovery record, but safe release could not be confirmed. Refresh its status and use authorized recovery.`;
    default: return `${prefix}has an external or unknown owner. Check Windows Disk Management or the app that assigned it; WinCommander left it unchanged.`;
  }
}
