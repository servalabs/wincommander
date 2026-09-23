import { Button, Tooltip } from "@/components/ui/bp";
import { useState } from "react";
import useBackend from "../../hooks/useBackend";
import VolumePropertiesDialog from "./VolumePropertiesDialog";
import TierGate from "../../components/shared/TierGate";
import { showSuccess, showError } from "../../utils/toast";
import './VolumeActionsMenu.css';

interface VolumeActionsMenuProps {
  letter: string;
  path: string | null;
  type: string;
  internalDrive?: number;
  accessible?: boolean;
  onDismounted: () => void;
}

function VolumeActionsMenu({ letter, path, type, internalDrive, accessible = true, onDismounted }: VolumeActionsMenuProps) {
  const { dismountVolume, getEncryptedVolumeStatus, openEncryptionVolume } = useBackend();

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

  const completeDismount = async (forced: boolean) => {
    const verificationError = await verifyDismounted();
    if (verificationError) {
      showError(verificationError, undefined, { kind: "notification" });
      return false;
    }
    onDismounted();
    showSuccess(`Volume ${driveLabel} ${forced ? "force-" : ""}dismounted.`);
    return true;
  };

  const handleDismount = async () => {
    setDismounting(true);
    try {
      // An unavailable volume can belong to another or stale Windows sign-in
      // and may not return the normal "in use" failure that used to expose
      // the second force-dismount step. Dismount its exact engine slot now,
      // then keep the row until fresh status confirms it has disappeared.
      const result = await dismountVolume(letter, true, internalDrive);
      if (!result.success) {
        const message = result.error || `Failed to dismount ${driveLabel}.`;
        showError(message, undefined, { kind: "notification" });
        return;
      }
      await completeDismount(true);
    } catch (e) {
      // Operational volume result → Notifications tab, not System Alerts.
      const message = e instanceof Error ? e.message : `Failed to dismount ${driveLabel}.`;
      showError(message, undefined, { kind: "notification" });
    } finally {
      setDismounting(false);
    }
  };

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
        <Tooltip content="Force dismount" position="top">
          <Button
            icon="eject"
            intent="danger"
            minimal
            small
            loading={dismounting}
            onClick={handleDismount}
            className="vol-danger-btn"
            aria-label={`Force dismount ${driveLabel}`}
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
