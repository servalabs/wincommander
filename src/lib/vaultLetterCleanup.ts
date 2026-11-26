// SPDX-License-Identifier: AGPL-3.0-or-later
const CATEGORIES = ["real_windows_volume", "active_wincommander_mount", "stale_wincommander_reservation", "duplicate_saved_entry", "system_or_policy_reserved_letter", "external_or_unknown_owner", "stale_drive_mapping"] as const;
const WINDOWS_MAPPINGS = ["current_session", "current_session_network", "other_session", "other_account"] as const;
const HOLDER_KINDS = ["vault_mount", "ram_disk", "windows_volume", "network_drive", "subst", "other"] as const;
const LOGON_STATES = ["machine", "active", "disconnected", "signed_out", "service", "unknown"] as const;
/** What the service saw holding a letter; `owner` is sent to administrators only. */
export interface VaultLetterHolder {
  kind: typeof HOLDER_KINDS[number];
  scope: "machine" | "session";
  logon_state: typeof LOGON_STATES[number];
  device_present: boolean;
  owner_is_caller?: boolean;
  owner?: string;
}
export interface VaultLetterReport {
  letter: string;
  category: typeof CATEGORIES[number];
  released: boolean;
  saved_entry: boolean;
  vault_policy_reservation?: boolean;
  vault_policy_owner_is_caller?: boolean;
  vault_policy_owner?: string;
  windows_mapping?: typeof WINDOWS_MAPPINGS[number];
  other_account_personal_vault?: boolean;
  holder?: VaultLetterHolder;
}
export interface VaultLetterCleanup { released: number; letters: VaultLetterReport[] }

const validName = (name: unknown) => typeof name === "string" && !!name.trim() && name.length <= 256 && !/\p{Cc}/u.test(name);
function validHolder(holder: VaultLetterHolder): boolean {
  return !!holder && typeof holder === "object" && HOLDER_KINDS.includes(holder.kind) && (holder.scope === "machine" || holder.scope === "session")
    && LOGON_STATES.includes(holder.logon_state) && typeof holder.device_present === "boolean"
    && (holder.owner_is_caller === undefined || typeof holder.owner_is_caller === "boolean")
    && (holder.owner === undefined || (validName(holder.owner) && holder.owner_is_caller === false));
}

export function readVaultLetterCleanup(value: unknown): VaultLetterCleanup {
  const result = value as VaultLetterCleanup | null;
  const invalid = () => { throw new Error("vault_drive_letter_cleanup_unverified"); };
  if (!result || !Number.isSafeInteger(result.released) || result.released < 0 || result.released > 26
    || !Array.isArray(result.letters) || result.letters.length > 26) return invalid();
  const seen = new Set<string>();
  for (const report of result.letters) {
    if (!report || typeof report.letter !== "string" || !/^[A-Z]$/.test(report.letter) || seen.has(report.letter) || !CATEGORIES.includes(report.category)
      || typeof report.released !== "boolean" || typeof report.saved_entry !== "boolean"
      || (report.vault_policy_reservation !== undefined && typeof report.vault_policy_reservation !== "boolean")
      || (report.vault_policy_reservation === true && report.category !== "system_or_policy_reserved_letter")
      || (report.vault_policy_owner_is_caller !== undefined && (typeof report.vault_policy_owner_is_caller !== "boolean" || report.vault_policy_reservation !== true))
      || (report.vault_policy_owner !== undefined && (typeof report.vault_policy_owner !== "string"
        || !report.vault_policy_owner.trim() || report.vault_policy_owner.length > 256
        || /\p{Cc}/u.test(report.vault_policy_owner) || report.vault_policy_owner_is_caller === undefined))
      || (report.windows_mapping !== undefined && (report.category !== "external_or_unknown_owner" || !WINDOWS_MAPPINGS.includes(report.windows_mapping)))
      || (report.other_account_personal_vault !== undefined && (typeof report.other_account_personal_vault !== "boolean" || report.category !== "active_wincommander_mount"))
      || (report.holder !== undefined && (!["external_or_unknown_owner", "stale_drive_mapping"].includes(report.category) || !validHolder(report.holder)))
      || (report.category === "stale_drive_mapping" && !report.released)
      || (report.released && (report.letter <= "C" || !["stale_wincommander_reservation", "stale_drive_mapping"].includes(report.category) || report.saved_entry))) return invalid();
    seen.add(report.letter);
  }
  if (result.letters.filter(report => report.released).length !== result.released) return invalid();
  return result;
}

const STATE_TEXT: Record<VaultLetterHolder["logon_state"], string> = {
  machine: "all accounts on this PC", active: "signed in", disconnected: "disconnected", signed_out: "signed out",
  service: "a Windows service logon", unknown: "session state unknown",
};
function holderAccount(holder: VaultLetterHolder): string {
  if (holder.scope === "machine") return "all accounts on this PC";
  const who = holder.owner_is_caller ? "your Windows account" : holder.owner ?? "another Windows account";
  return `${who} (${STATE_TEXT[holder.logon_state]})`;
}
function leftOverFrom(holder: VaultLetterHolder): string {
  if (holder.kind === "ram_disk") return "a removed RAM disk";
  if (holder.scope === "machine") return "a closed vault";
  if (holder.owner_is_caller) return "your closed vault";
  return `${holder.owner ?? "another Windows account"}'s closed vault`;
}

function holderMessage(prefix: string, letter: string, holder: VaultLetterHolder): string {
  const account = holderAccount(holder);
  switch (holder.kind) {
    case "ram_disk": return holder.device_present
      ? `${prefix}is the WinCommander RAM disk. WinCommander left it unchanged; choose another letter.`
      : `${prefix}still points to a removed RAM disk and could not be freed. Check again.`;
    case "vault_mount": {
      if (!holder.device_present) return `${prefix}still points to a closed vault for ${account} and could not be freed. Check again.`;
      const step = holder.owner_is_caller ? "Dismount it in Secure Storage" : holder.scope === "machine"
        ? "Dismount it in Secure Storage or in the app that mounted it" : "Dismount it from that account";
      return `${prefix}is held by a mounted encrypted volume for ${account}. ${step}, or choose another letter.`;
    }
    case "windows_volume": return `${prefix}is a Windows disk volume for ${account}. Choose another letter.`;
    case "network_drive": return `${prefix}is a network drive mapped for ${account}. WinCommander never removes network drives; disconnect it in that account or choose another letter.`;
    case "subst": return `${prefix}is a folder mapped with subst for ${account}. Remove it in that account (subst ${letter}: /d) or choose another letter.`;
    default: return `${prefix}is mapped to a Windows device for ${account}. WinCommander left it unchanged; choose another letter.`;
  }
}

/** One-line toast, for example "Freed T: (left over from SERVAULT\King's closed vault)." */
export function freedLettersSummary(reports: VaultLetterReport[]): string | null {
  const freed = reports.filter(report => report.released).map(report => report.holder
    ? `${report.letter}: (left over from ${leftOverFrom(report.holder)})` : `${report.letter}:`);
  return freed.length ? `Freed ${freed.join(", ")}.` : null;
}

export function vaultLetterReportMessage(report: VaultLetterReport): string {
  const prefix = `${report.letter}: `;
  if (report.letter <= "C") return `${prefix}is outside the D:–Z: range supported by Vault cleanup.`;
  if (report.released && report.holder) return `${prefix}was freed. It was left over from ${leftOverFrom(report.holder)}.`;
  if (report.released) return `${prefix}a confirmed stale WinCommander reservation was released.`;
  if (report.holder) return holderMessage(prefix, report.letter, report.holder);
  switch (report.category) {
    case "real_windows_volume": return `${prefix}is in use by a Windows volume or device. This is normal; choose a free letter for the Vault.`;
    case "active_wincommander_mount": return report.other_account_personal_vault
      ? `${prefix}is in use by another Windows account’s personal Vault.`
      : `${prefix}is held by an active Vault or recovery operation. Check Secure Storage; finish the operation or use its authorized recovery action.`;
    case "duplicate_saved_entry": return `${prefix}is selected by duplicate saved entries. Edit or remove the duplicate shortcuts; no Windows mapping was changed.`;
    case "system_or_policy_reserved_letter": {
      if (report.vault_policy_reservation) {
        const owner = report.vault_policy_owner_is_caller === true ? "your Windows account"
          : report.vault_policy_owner ?? (report.vault_policy_owner_is_caller === false ? "another Windows account" : null);
        return owner
          ? `${prefix}is reserved for ${owner} by a saved Vault policy. Check Vault Access to review the reservation.`
          : `${prefix}is reserved by a saved Vault policy and cannot be used by another Vault. Check Vault Access to review the reservation.`;
      }
      return report.saved_entry
        ? `${prefix}is reserved by a saved Quick Mount entry. Remove or edit that entry to release its preference.`
        : `${prefix}is protected by Windows or Vault policy and cannot be released here.`;
    }
    case "stale_wincommander_reservation": return `${prefix}has a WinCommander recovery record, but safe release could not be confirmed. Refresh its status and use authorized recovery.`;
    default:
      switch (report.windows_mapping) {
        case "current_session_network": return `${prefix}is a network drive mapped in your Windows session.`;
        case "current_session": return `${prefix}is mapped in your Windows session outside WinCommander.`;
        case "other_session": return `${prefix}is mapped in another session for your Windows account.`;
        case "other_account": return `${prefix}is mapped by another Windows account.`;
        default: return `${prefix}is in use or reserved outside the verified Vault records. Windows did not confirm who owns it. WinCommander left it unchanged.`;
      }
  }
}
