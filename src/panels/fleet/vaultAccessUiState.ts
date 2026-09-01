import type { FleetAccessDirectory } from "./accessControlTypes";
import { vaultEntryResultLabel } from "./vaultAccessPresentation";
import type { VaultAuthorizedEntry, VaultEntryResult, VaultMountEntryResult } from "./vaultAccessTypes";

/**
 * The mount broker returns the authoritative lifecycle result for one entry.
 * Patch only that caller-filtered row; policy or authorization changes still
 * require a full service refresh.
 */
export function patchAuthorizedEntriesFromMountResult(
  entries: VaultAuthorizedEntry[],
  result: VaultMountEntryResult,
): VaultAuthorizedEntry[] {
  return entries.map(entry => entry.entry_id === result.entry_id
    ? {
      ...entry,
      mount_state: result.state,
      drive_letter: result.state === "mounted" ? result.drive_letter : null,
    }
    : entry);
}

export type VaultPrincipalKind = "group" | "user" | "unverified";

export interface VaultPrincipalOption {
  /** Bare local name emitted into the grant. Resolved by the service via
   * LookupAccountNameW(NULL, ...) — never a MACHINE\ prefixed name. */
  principalName: string;
  /** Friendly text shown in the picker. */
  label: string;
  kind: VaultPrincipalKind;
}

/**
 * An access-control group has two names — a friendly `name` ("Sales") and the
 * Windows group `localGroup` ("WC_Sales"). Only `localGroup` is a resolvable
 * Windows principal, matching the convention `resolveLegacyPrincipal` already
 * uses. The friendly name is display-only and must never be emitted.
 */
export function vaultPrincipalDirectoryOptions(directory: FleetAccessDirectory): VaultPrincipalOption[] {
  const groups: VaultPrincipalOption[] = directory.groups.map(group => ({
    principalName: group.localGroup,
    label: group.name,
    kind: "group",
  }));
  const users: VaultPrincipalOption[] = directory.users
    .filter(user => user.isAvailable !== false)
    .map(user => ({
      principalName: user.username,
      label: user.displayName || user.username,
      kind: "user",
    }));
  return [...groups, ...users];
}

/**
 * A saved grant may name a principal the current directory doesn't know about
 * (a domain account, a built-in group, a directory group renamed since it was
 * granted). Preserve it as unverified instead of silently dropping it.
 */
export function resolveVaultPrincipalOption(
  principalName: string,
  options: VaultPrincipalOption[],
): VaultPrincipalOption {
  const key = principalName.trim().toLocaleLowerCase();
  const matched = options.find(option => option.principalName.trim().toLocaleLowerCase() === key);
  return matched ?? { principalName, label: principalName, kind: "unverified" };
}

/** Directory options plus the current grant's principal, pinned in as an
 * unverified entry when the directory doesn't already resolve it. */
export function vaultPrincipalSelectOptions(
  currentPrincipalName: string,
  directory: FleetAccessDirectory,
): VaultPrincipalOption[] {
  const options = vaultPrincipalDirectoryOptions(directory);
  if (!currentPrincipalName.trim()) return options;
  const current = resolveVaultPrincipalOption(currentPrincipalName, options);
  return current.kind === "unverified" ? [current, ...options] : options;
}

export interface VaultMountGate {
  canMount: boolean;
  /** Administrator-facing reason shown when Mount is disabled; null when enabled. */
  disabledReason: string | null;
}

/**
 * Mount must never be offered for a policy that never reached the service:
 * `authorized` reflects the caller-filtered projection the service actually
 * granted, not the (possibly dirty, never-applied) draft entry being edited.
 */
export function vaultMountGate(params: {
  authorized: VaultAuthorizedEntry | undefined;
  entryResult: VaultEntryResult | undefined;
  draftDirty: boolean;
}): VaultMountGate {
  const { authorized, entryResult, draftDirty } = params;
  if (!authorized) {
    return {
      canMount: false,
      disabledReason: draftDirty
        ? "Save vault settings before mounting — this draft has not been applied yet."
        : "This Windows account is not authorized to mount this vault.",
    };
  }
  if (entryResult && entryResult !== "applied") {
    return { canMount: false, disabledReason: vaultEntryResultLabel(entryResult) };
  }
  return { canMount: true, disabledReason: null };
}
