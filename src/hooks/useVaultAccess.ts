import { invoke } from "@tauri-apps/api/core";
import { useCallback } from "react";
import type {
  AccessGroupReconcileRequest, AccessGroupReconcileResponse, VaultAccessDirectory,
  VaultSaveAccessDirectoryResponse,
} from "@/panels/fleet/accessControlTypes";
import type { VaultAccessCapabilities, VaultAuthorizedEntry, VaultMountEntryResult, VaultOwnerPolicyFragment, VaultOwnerPrincipalList, VaultVolumeRole } from "@/panels/fleet/vaultAccessTypes";
import { newDiagnosticOperationId } from "@/lib/diagnostics";
import { afterVaultMutation } from "@/lib/vaultChangeEvents";

export { FLEET_VAULTS_CHANGED_EVENT } from "@/lib/vaultChangeEvents";

/** Typed renderer boundary for the service-owned Vault Access policy. */
export default function useVaultAccess<Policy, Status>() {
  const getPolicy = useCallback(
    () => invoke<Policy | null>("get_vault_access_policy"),
    [],
  );
  const getOwnerPolicyFragment = useCallback(
    () => invoke<VaultOwnerPolicyFragment>("get_vault_access_policy"),
    [],
  );
  const getStatus = useCallback(
    () => invoke<Status>("get_vault_access_status"),
    [],
  );
  const applyPolicy = useCallback(
    (policy: Policy, operationId?: string) =>
      afterVaultMutation(() => invoke<Status>("apply_vault_access_policy", {
        policy,
        diagnosticOperationId: operationId,
      })),
    [],
  );
  const applyOwnerPolicyFragment = useCallback(
    async (fragment: VaultOwnerPolicyFragment, _operationId?: string) => {
      // This adapter accepts the fragment itself, not a renderer-defined
      // wrapper. Its service side authenticates the caller and derives every
      // authoritative path/identity observation.
      return afterVaultMutation(() => invoke<Status>("apply_vault_owner_policy_fragment", { policy: fragment }));
    },
    [],
  );
  // This is deliberately different from applying an empty policy. The
  // service removes only the selected record and leaves every Windows ACL
  // untouched.
  const forgetPolicy = useCallback(
    (entryId: string, policyId: string, expectedVersion: number, operationId?: string) =>
      afterVaultMutation(() => invoke<void>("forget_vault_access_entry_policy_only", {
        entryId,
        policyId,
        expectedVersion,
        diagnosticOperationId: operationId,
      })),
    [],
  );
  const mountEntry = useCallback(
    (entryId: string, password: string, volumeRole: VaultVolumeRole, hiddenProtectionPassword?: string, operationId = newDiagnosticOperationId("vault")) =>
      afterVaultMutation(() => invoke<VaultMountEntryResult>("vault_mount_entry", { entryId, password, volumeRole, hiddenProtectionPassword, diagnosticOperationId: operationId })),
    [],
  );
  const unmountEntry = useCallback(
    (entryId: string, operationId = newDiagnosticOperationId("vault")) => afterVaultMutation(() => invoke<VaultMountEntryResult>("vault_unmount_entry", { entryId, diagnosticOperationId: operationId })),
    [],
  );
  const listAuthorizedEntries = useCallback(
    () => invoke<VaultAuthorizedEntry[]>("vault_list_authorized_entries"),
    [],
  );
  const getCapabilities = useCallback(
    () => invoke<VaultAccessCapabilities>("get_vault_access_capabilities"),
    [],
  );
  const listOwnerPrincipals = useCallback(
    () => invoke<VaultOwnerPrincipalList>("vault_list_known_principals"),
    [],
  );
  const reconcileAccessGroups = useCallback(
    (groups: AccessGroupReconcileRequest[]) =>
      invoke<AccessGroupReconcileResponse>("reconcile_vault_access_groups", { groups }),
    [],
  );
  const getAccessDirectory = useCallback(
    () => invoke<VaultAccessDirectory>("get_vault_access_directory"),
    [],
  );
  const saveAccessDirectory = useCallback(
    (directory: VaultAccessDirectory) =>
      invoke<VaultSaveAccessDirectoryResponse>("save_vault_access_directory", { directory }),
    [],
  );

  return {
    getPolicy, getOwnerPolicyFragment, getStatus, applyPolicy, applyOwnerPolicyFragment, forgetPolicy, mountEntry, unmountEntry, listAuthorizedEntries, getCapabilities, listOwnerPrincipals,
    reconcileAccessGroups, getAccessDirectory, saveAccessDirectory,
  };
}
