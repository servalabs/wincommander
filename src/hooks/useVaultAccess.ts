import { invoke } from "@tauri-apps/api/core";
import { useCallback } from "react";
import type {
  AccessGroupReconcileRequest, AccessGroupReconcileResponse, VaultAccessDirectory,
  VaultSaveAccessDirectoryResponse,
} from "@/panels/fleet/accessControlTypes";
import type { VaultAccessCapabilities, VaultAuthorizedEntry, VaultMountEntryResult, VaultOwnerPolicyFragment, VaultOwnerPrincipalList, VaultVolumeRole } from "@/panels/fleet/vaultAccessTypes";
import { newDiagnosticOperationId } from "@/lib/diagnostics";

/** Raised only after the service has durably accepted a Fleet Vault change.
 * RightSidebar re-reads its caller-filtered, opaque-ID projection; no policy
 * details or container locations travel with this notification. */
export const FLEET_VAULTS_CHANGED_EVENT = "fleet-vaults-changed";

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
      invoke<Status>("apply_vault_access_policy", {
        policy,
        diagnosticOperationId: operationId,
      }),
    [],
  );
  const applyOwnerPolicyFragment = useCallback(
    async (fragment: VaultOwnerPolicyFragment, _operationId?: string) => {
      // This adapter accepts the fragment itself, not a renderer-defined
      // wrapper. Its service side authenticates the caller and derives every
      // authoritative path/identity observation.
      const result = await invoke<Status>("apply_vault_owner_policy_fragment", { policy: fragment });
      window.dispatchEvent(new Event(FLEET_VAULTS_CHANGED_EVENT));
      return result;
    },
    [],
  );
  // This is deliberately different from applying an empty policy. The
  // service removes only the selected record and leaves every Windows ACL
  // untouched.
  const forgetPolicy = useCallback(
    (entryId: string, policyId: string, expectedVersion: number, operationId?: string) =>
      invoke<void>("forget_vault_access_entry_policy_only", {
        entryId,
        policyId,
        expectedVersion,
        diagnosticOperationId: operationId,
      }),
    [],
  );
  const mountEntry = useCallback(
    (entryId: string, password: string, volumeRole: VaultVolumeRole, hiddenProtectionPassword?: string, operationId = newDiagnosticOperationId("vault")) =>
      invoke<VaultMountEntryResult>("vault_mount_entry", { entryId, password, volumeRole, hiddenProtectionPassword, diagnosticOperationId: operationId }),
    [],
  );
  const unmountEntry = useCallback(
    (entryId: string, operationId = newDiagnosticOperationId("vault")) => invoke<VaultMountEntryResult>("vault_unmount_entry", { entryId, diagnosticOperationId: operationId }),
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
