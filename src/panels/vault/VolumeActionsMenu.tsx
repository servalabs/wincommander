import { Button, Tooltip } from "@/components/ui/bp";
import { useState } from "react";
import useBackend from "../../hooks/useBackend";
import useVaultAccess from "@/hooks/useVaultAccess";
import type { VaultMountEntryResult } from "@/panels/fleet/vaultAccessTypes";
import { vaultMountResultLabel } from "@/panels/fleet/vaultAccessTypes";
import type { SecureStorageDismountTarget } from "./mountDismountTarget";
import VolumePropertiesDialog from "./VolumePropertiesDialog";
import TierGate from "../../components/shared/TierGate";
import { showSuccess, showError } from "../../utils/toast";
import './VolumeActionsMenu.css';

interface VolumeActionsMenuProps {
  letter: string;
  path: string | null;
  type: string;
  accessible?: boolean;
  dismountTarget: SecureStorageDismountTarget;
  dismountAuthorityLoading?: boolean;
  onDismounted: () => Promise<unknown> | void;
}

function VolumeActionsMenu({ letter, path, type, accessible = true, dismountTarget, dismountAuthorityLoading = false, onDismounted }: VolumeActionsMenuProps) {
  const { getEncryptedVolumeStatus, openEncryptionVolume } = useBackend();
  const { dismountPersonalMount, unmountEntry } = useVaultAccess<never, never>();

  const [dismounting, setDismounting] = useState(false);
  const [propertiesOpen, setPropertiesOpen] = useState(false);
  const driveLabel = letter.endsWith(":") ? letter : `${letter}:`;

  const verifyDismounted = async (): Promise<string | null> => {
    const normalizedLetter = driveLabel.toUpperCase();
    for (let attempt = 0; attempt < 4; attempt += 1) {
      const status = await getEncryptedVolumeStatus();
      if (!status.success || !status.data || !Array.isArray(status.data.volumes)) {
        return "The dismount command completed, but WinCommander could not verify this logon session's driver status.";
      }
      const stillMounted = status.data.volumes.some(
        volume => volume.letter.toUpperCase() === normalizedLetter,
      );
      if (!stillMounted) return null;
      if (attempt < 3) await new Promise(resolve => window.setTimeout(resolve, 250));
    }
    return `${driveLabel} is still reported as mounted in this logon session.`;
  };

  const completeDismount = async () => {
    const verificationError = await verifyDismounted();
    if (verificationError) {
      showError(verificationError, undefined, { kind: "notification" });
      return false;
    }
    showSuccess(`Volume ${driveLabel} dismounted.`);
    return true;
  };

  const handleDismount = async () => {
    if (!dismountTarget) return;
    setDismounting(true);
    try {
      // Only the authenticated service ledger decides whether this caller may
      // close the volume. Fleet entries are addressed by policy entry id;
      // ordinary entries use only the service-owned internal slot.
      const result: VaultMountEntryResult = dismountTarget.kind === "fleet"
        ? await unmountEntry(dismountTarget.entryId)
        : await dismountPersonalMount(dismountTarget.internalDrive);
      if (result.state !== "unmounted") {
        showError(vaultMountResultLabel(result) || `Failed to dismount ${driveLabel}.`, undefined, { kind: "notification" });
        return;
      }
      await completeDismount();
    } catch (e) {
      // Operational volume result → Notifications tab, not System Alerts.
      const message = e instanceof Error ? e.message : `Failed to dismount ${driveLabel}.`;
      showError(message, undefined, { kind: "notification" });
    } finally {
      setDismounting(false);
      // A denied, failed, or delayed service result can still coincide with a
      // registry/status transition; refresh the service ledger and drive view
      // after every request rather than leaving a stale row behind.
      try { await onDismounted(); } catch { /* the operation result stays primary */ }
    }
  };

  const dismountUnavailable = dismountAuthorityLoading || dismountTarget === null;
  const dismountTooltip = dismountAuthorityLoading
    ? "Checking this volume’s service authorization…"
    : dismountTarget === null
      ? "Dismount is unavailable until the security service confirms this volume’s owner."
      : dismountTarget.kind === "fleet"
        ? "Dismount this Fleet Vault through its saved permission."
        : "Dismount this Secure Storage volume through the local security service.";

  const handleOpen = async () => {
    await openEncryptionVolume(letter);
  };

  return (
    <div className="flex items-center gap-1 flex-shrink-0">
      <Tooltip content="Open in Explorer" position="top">
        <Button
          icon="folder-open"
          minimal
          small
          onClick={handleOpen}
          disabled={!accessible}
          className="vol-inline-btn"
          aria-label={accessible ? `Open ${driveLabel} in Explorer` : `${driveLabel} is unavailable in this Windows sign-in`}
        />
      </Tooltip>

      <Tooltip content="Properties" position="top">
        <Button
          icon="info-sign"
          minimal
          small
          onClick={() => setPropertiesOpen(true)}
          className="vol-inline-btn"
          aria-label={`View properties for ${driveLabel}`}
        />
      </Tooltip>

      <TierGate tier="paid" featureLabel="Encrypted volumes">
        <Tooltip content={dismountTooltip} position="top">
          <Button
            icon="eject"
            intent="danger"
            minimal
            small
            loading={dismounting}
            disabled={dismountUnavailable}
            onClick={handleDismount}
            className="vol-danger-btn"
            aria-label={dismountUnavailable ? `Dismount unavailable for ${driveLabel}` : `Dismount ${driveLabel}`}
          />
        </Tooltip>
      </TierGate>

      <VolumePropertiesDialog
        isOpen={propertiesOpen}
        onClose={() => setPropertiesOpen(false)}
        letter={letter}
        path={path}
        type={type}
      />
    </div>
  );
}

export default VolumeActionsMenu;
