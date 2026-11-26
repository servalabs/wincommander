import type { VaultMenuItem } from "@/components/shared/VaultActionMenu";
import { DISMOUNT_FIRST_HINT, type VaultFileActions, type VaultFileTarget } from "@/lib/vaultFileActions";

export interface VaultPolicyMenuInput {
  /** Null without Pro: both file actions check the password in the Pro engine. */
  fileActions: VaultFileActions | null;
  target: VaultFileTarget;
  /** Mounted or awaiting cleanup. */
  mounted: boolean;
  mountStateUnknown: boolean;
  saving: boolean;
  canEditEntry: boolean;
  /** Owner or a write grant; an administrator outside the share is refused by the service too. */
  canChangeFile: boolean;
  canRemoveEntry: boolean;
  canRepairSharedAccess: boolean;
  onRepair: () => void;
  onForget: () => void;
  onRemove: () => void;
}

export const VAULT_NOT_SHARED_FOR_EDIT = "Only this vault's owner, or an account it is shared with for editing, can change its password or rename it.";

const firstReason =(...checks: Array<[boolean, string]>) => checks.find(([blocked]) => blocked)?.[1] ?? null;

/** The less frequent actions of a saved Vault row, so the row keeps one line. */
export function vaultPolicyMenuItems(input: VaultPolicyMenuInput): VaultMenuItem[] {
  const { fileActions, target, mounted, mountStateUnknown, saving, canEditEntry, canRemoveEntry } = input;
  const fileBlocked = firstReason(
    [!input.canChangeFile, VAULT_NOT_SHARED_FOR_EDIT],
    [mounted, DISMOUNT_FIRST_HINT],
    [mountStateUnknown, "Vault mount status is unavailable. Refresh first."],
  );
  const policyBlocked = (deniedReason: string) => firstReason(
    [saving, "Wait for the Vault settings to finish saving."],
    [mounted, "Dismount this Vault before changing its policy."],
    [mountStateUnknown, "Vault mount status is unavailable. Refresh before changing this policy."],
    [!canEditEntry, deniedReason],
  );
  return [
    ...(fileActions ? [
      { label: "Change Password…", onSelect: () => fileActions.changePassword(target), disabledReason: fileBlocked },
      { label: "Rename…", onSelect: () => fileActions.rename(target), disabledReason: fileBlocked },
    ] : []),
    ...(input.canRepairSharedAccess ? [
      { label: "Repair shared access", onSelect: input.onRepair, disabledReason: policyBlocked("This Windows account cannot repair this Vault policy."), hasSeparatorBefore: Boolean(fileActions) },
      { label: "Forget policy…", onSelect: input.onForget, disabledReason: policyBlocked("This Windows account cannot change this Vault policy.") },
    ] : []),
    {
      label: "Remove policy",
      onSelect: input.onRemove,
      isDanger: true,
      hasSeparatorBefore: Boolean(fileActions) || input.canRepairSharedAccess,
      disabledReason: firstReason(
        [mounted, "Dismount this Vault before removing its policy."],
        [mountStateUnknown, "Vault mount status is unavailable. Refresh before removing this policy."],
        [!canRemoveEntry, "This Windows account cannot remove this Vault policy."],
      ),
    },
  ];
}
