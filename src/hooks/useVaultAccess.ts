import { invoke } from "@tauri-apps/api/core";
import { useCallback } from "react";
import type {
  AccessGroupReconcileRequest, AccessGroupReconcileResponse, VaultAccessDirectory,
  VaultSaveAccessDirectoryResponse,
} from "@/panels/fleet/accessControlTypes";
import type { VaultAccessCapabilities, VaultAuthorizedEntry, VaultMountEntryResult, VaultVolumeRole } from "@/panels/fleet/vaultAccessTypes";
import { newDiagnosticOperationId } from "@/lib/diagnostics";

/** Typed renderer boundary for the service-owned Vault Access policy. */
export default function useVaultAccess<Policy, Status>() {
  const getPolicy = useCallback(
    () => invoke<Policy | null>("get_vault_access_policy"),
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
    getPolicy, getStatus, applyPolicy, mountEntry, unmountEntry, listAuthorizedEntries, getCapabilities,
    reconcileAccessGroups, getAccessDirectory, saveAccessDirectory,
  };
}
