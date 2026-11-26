import { Button, Tooltip } from "@/components/ui/bp";
import { DISMOUNT_FIRST_HINT, type VaultFileActions } from "@/lib/vaultFileActions";

/** One icon for a mounted row: both actions need the vault dismounted, and the row has no room for two. */
export function MountedVaultFileButton({ driveLabel }: { driveLabel: string }) {
  return (
    <Tooltip content={`Change Password and Rename: ${DISMOUNT_FIRST_HINT}`} position="top">
      {/* A disabled button gets no pointer events, so the tooltip listens on this wrapper. */}
      <span className="inline-flex" tabIndex={0} aria-label={`Change password or rename ${driveLabel}. ${DISMOUNT_FIRST_HINT}`}>
        <Button icon="key" minimal small disabled className="vol-inline-btn" tabIndex={-1} />
      </span>
    </Tooltip>
  );
}

/** Entry point for a container no list shows, such as a file WinCommander has no record of. */
export function VaultFileLinks({ actions }: { actions: VaultFileActions }) {
  return (
    <div className="vault-file-links">
      {actions.changePassword && (
        <Button minimal small icon="key" text="Change password of a vault file…" onClick={() => actions.changePassword?.({ path: null })} />
      )}
      <Button minimal small icon="edit" text="Rename a vault file…" onClick={() => actions.rename({ path: null })} />
    </div>
  );
}
