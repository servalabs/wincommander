/** Fixed service categories only: never display raw account, path or transport details. */
export function accessGroupSaveFailure(cause: unknown): string {
  const detail = (cause instanceof Error ? cause.message : String(cause ?? "")).toLowerCase();
  if (detail.includes("vault_not_authorized") || detail.includes("vault_fleet_group_required")) {
    return "These access groups were not saved. You must already belong to a Fleet group to change or remove it; being a Windows administrator does not grant membership.";
  }
  if (detail.includes("vault_policy_mounted")) {
    return "These access groups were not saved because an affected Vault is mounted. Ask an authorized group member to dismount it before changing membership.";
  }
  if (detail.includes("vault_group_in_use")) {
    return "This group is still assigned to a Vault. An authorized member must update that unmounted Vault's permissions before the group can be deleted or its Windows group name changed.";
  }
  if (detail.includes("vault_group_name_conflict")) {
    return "That Windows group name is already in use. Choose a different name for the new group; WinCommander will not take over an existing Windows group.";
  }
  if (detail.includes("vault_legacy_group_wire_retired")) {
    return "Reload Fleet Access control before saving groups. This older group-saving method is no longer supported.";
  }
  return "Access groups could not be saved. Refresh the saved groups and Windows users, then review your changes before retrying.";
}
