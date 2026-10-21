import type { EncryptionStatus } from "@/hooks/useBackend";
import type { VaultSyncManagementResult } from "@/types/generated/ipc";
import { acceptsVaultSyncMountReceipt } from "./personalVaultSyncManagement";
import { quickMountExplorerTargetForDrive, type QuickMountExplorerTarget } from "./quickMountObservation";
import { vaultSyncRecoveryChoice, type PersonalVaultSyncEnrollment } from "./vaultSyncRecovery";
import type { VaultSyncNotice } from "./vaultSyncWarning";

interface Inspection {
  readStatus: () => Promise<{ success: boolean; data?: EncryptionStatus | null }>;
  list: (slot: number) => Promise<VaultSyncManagementResult>;
  inspect: (slot: number, receipt: string) => Promise<PersonalVaultSyncEnrollment>;
}
export type VaultSyncCheck = { kind: "clear" | "stale" } | { kind: "recovery"; notice: VaultSyncNotice; target: QuickMountExplorerTarget };

function sameMount(current: QuickMountExplorerTarget | null, expected: QuickMountExplorerTarget): boolean {
  return current !== null && current.receipt === expected.receipt && current.internalDrive === expected.internalDrive
    && current.containerPath === expected.containerPath;
}

/** Inspect only the caller's still-mounted Vault; never enroll or recreate a folder. */
export async function inspectPostMountSync(notice: VaultSyncNotice, api: Inspection): Promise<VaultSyncCheck> {
  const first = await api.readStatus();
  const target = first.success && first.data ? quickMountExplorerTargetForDrive(first.data, notice.drive) : null;
  if (!target) throw new Error("vault_mount_state_unknown");
  const current = async () => {
    const state = await api.readStatus();
    return state.success && state.data ? sameMount(quickMountExplorerTargetForDrive(state.data, notice.drive), target) : false;
  };
  const listed = await api.list(target.internalDrive);
  if (!await current()) return { kind: "stale" };
  if (!acceptsVaultSyncMountReceipt("", listed.mount_receipt)) throw new Error("vault_sync_mount_changed");
  if (!listed.syncthing_enabled) throw new Error("vault_syncthing_not_enabled");
  if (!listed.folders.some(folder => folder.recovery_required)) return { kind: "clear" };
  const result = await api.inspect(target.internalDrive, listed.mount_receipt);
  if (!await current()) return { kind: "stale" };
  if (!result.recovery_required) return { kind: "clear" };
  const recovery = vaultSyncRecoveryChoice(target.internalDrive, result.recovery_roots);
  if (!recovery) throw new Error("vault_broker_rejected");
  return { kind: "recovery", notice: { ...notice, recovery }, target };
}
