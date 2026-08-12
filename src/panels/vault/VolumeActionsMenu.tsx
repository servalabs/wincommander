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
  onDismounted: () => void;
}

function VolumeActionsMenu({ letter, path, type, onDismounted }: VolumeActionsMenuProps) {
  const { dismountVolume, openEncryptionVolume } = useBackend();
  const { refreshVault } = useAppState();

  const [dismounting, setDismounting] = useState(false);
  const [propertiesOpen, setPropertiesOpen] = useState(false);
  const [forceConfirmOpen, setForceConfirmOpen] = useState(false);
  const driveLabel = letter.endsWith(":") ? letter : `${letter}:`;

  const handleDismount = async () => {
    setDismounting(true);
    try {
      const result = await dismountVolume(letter);
      if (!result.success) {
        showError(result.error || `Failed to dismount ${driveLabel}.`, undefined, { kind: "notification" });
        setForceConfirmOpen(true);
        return;
      }
      await refreshVault(true);
      onDismounted();
      showSuccess(`Volume ${driveLabel} dismounted.`);
    } catch (e) {
      // Operational volume result → Notifications tab, not System Alerts.
      showError(e instanceof Error ? e.message : `Failed to dismount ${driveLabel}.`, undefined, { kind: "notification" });
      setForceConfirmOpen(true);
    } finally {
      setDismounting(false);
    }
  };

  const handleForceDismount = async () => {
    setDismounting(true);
    try {
      const result = await dismountVolume(letter, true);
      if (!result.success) {
        showError(result.error || `Failed to force-dismount ${driveLabel}.`, undefined, { kind: "notification" });
        return;
      }
      setForceConfirmOpen(false);
      await refreshVault(true);
      onDismounted();
      showSuccess(`Volume ${driveLabel} force-dismounted.`);
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
