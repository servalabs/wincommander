// SPDX-License-Identifier: AGPL-3.0-or-later
import { validatePersonalVaultSyncFolders } from "./personalVaultSyncFeedback";
export const DEFAULT_VAULT_SYNC_LABEL = "WinCommander Vault";
export interface VaultSyncDraft { path: string; label: string }
export function vaultSyncLabel(label?: string): string { return label?.trim() || DEFAULT_VAULT_SYNC_LABEL; }
export function acceptsVaultSyncMountReceipt(current: string, incoming: unknown): incoming is string {
  return typeof incoming === "string" && /^[a-f0-9]{64}$/.test(incoming) && (!current || incoming === current);
}
export function validVaultSyncLabel(label: string): boolean {
  return Boolean(label.trim()) && new TextEncoder().encode(label).length <= 128 && !/[\x00-\x1f\x7f-\x9f]/.test(label);
}
export function validateVaultSyncDrafts(drafts: VaultSyncDraft[], existingPaths: string[]) {
  if (drafts.some(draft => !validVaultSyncLabel(vaultSyncLabel(draft.label)))) return { ok: false as const, message: "Give each sync folder a name of up to 128 bytes without control characters." };
  return validatePersonalVaultSyncFolders([...existingPaths, ...drafts.map(draft => draft.path)]);
}
