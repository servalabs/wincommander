import { Button, Dialog, Tooltip } from "@/components/ui/bp";
import { useState } from "react";
import useBackend from "../../hooks/useBackend";
import { useAppState } from "../../context/AppContext";
import VolumePropertiesDialog from "./VolumePropertiesDialog";
import TierGate from "../../components/shared/TierGate";
import { showSuccess, showError } from "../../utils/toast";
import './VolumeActionsMenu.css';

interface VolumeActionsMenuProps {
  letter: string;
  path: string | null;
  type: string;
  internalDrive?: number;
  onDismounted: () => void;
}

function VolumeActionsMenu({ letter, path, type, internalDrive, onDismounted }: VolumeActionsMenuProps) {
  const { dismountVolume, getEncryptedVolumeStatus, openEncryptionVolume } = useBackend();
  const { refreshVault } = useAppState();

  const [dismounting, setDismounting] = useState(false);
  const [propertiesOpen, setPropertiesOpen] = useState(false);
  const [forceConfirmOpen, setForceConfirmOpen] = useState(false);
  const [dismountFailure, setDismountFailure] = useState("");
  const driveLabel = letter.endsWith(":") ? letter : `${letter}:`;

  const requiresForce = (message: string) => /dismount error 29|in use|busy/i.test(message);

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
      setDismountFailure(verificationError);
      setForceConfirmOpen(verificationError.includes("still reported as mounted"));
      showError(verificationError, undefined, { kind: "notification" });
      return false;
    }
    setDismountFailure("");
    setForceConfirmOpen(false);
    await refreshVault(true);
    onDismounted();
    showSuccess(`Volume ${driveLabel} ${forced ? "force-" : ""}dismounted.`);
    return true;
  };

  const handleDismount = async () => {
    setDismounting(true);
    try {
      const result = await dismountVolume(letter, false, internalDrive);
      if (!result.success) {
        const message = result.error || `Failed to dismount ${driveLabel}.`;
        setDismountFailure(message);
        setForceConfirmOpen(requiresForce(message));
        showError(message, undefined, { kind: "notification" });
        return;
      }
      await completeDismount(false);
    } catch (e) {
      // Operational volume result → Notifications tab, not System Alerts.
      const message = e instanceof Error ? e.message : `Failed to dismount ${driveLabel}.`;
      setDismountFailure(message);
      setForceConfirmOpen(requiresForce(message));
      showError(message, undefined, { kind: "notification" });
    } finally {
      setDismounting(false);
    }
  };

  const handleForceDismount = async () => {
    setDismounting(true);
    try {
      const result = await dismountVolume(letter, true, internalDrive);
      if (!result.success) {
        showError(result.error || `Failed to force-dismount ${driveLabel}.`, undefined, { kind: "notification" });
        return;
      }
      await completeDismount(true);
    } catch (e) {
      showError(e instanceof Error ? e.message : `Failed to force-dismount ${driveLabel}.`, undefined, { kind: "notification" });
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
          className="vol-inline-btn"
          aria-label={`Open ${driveLabel} in Explorer`}
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
        <Tooltip content="Dismount" position="top">
          <Button
            icon="eject"
            intent="danger"
            minimal
            small
            loading={dismounting}
            onClick={handleDismount}
            className="vol-danger-btn"
            aria-label={`Dismount ${driveLabel}`}
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
      <Dialog
        isOpen={forceConfirmOpen}
        onClose={() => setForceConfirmOpen(false)}
        title={`Force-dismount ${driveLabel}?`}
      >
        <div className="wc-dialog-body">
          <p>The normal dismount was refused because the volume may still be in use. Forcing it can lose unwritten data.</p>
          {dismountFailure && <p className="mt-3 text-[13px] leading-5 text-[var(--text-dim)]">{dismountFailure}</p>}
        </div>
        <div className="mount-dialog-footer">
          <Button text="CANCEL" minimal onClick={() => setForceConfirmOpen(false)} />
          <Button intent="danger" text="FORCE DISMOUNT" loading={dismounting} onClick={handleForceDismount} />
        </div>
      </Dialog>
    </div>
  );
}

export default VolumeActionsMenu;
