import { Button, Tooltip } from "@/components/ui/bp";
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

  const handleDismount = async () => {
    setDismounting(true);
    try {
      await dismountVolume(letter);
      await refreshVault(true);
      onDismounted();
      showSuccess(`Volume ${letter}: dismounted.`);
    } catch (e) {
      // Operational volume result → Notifications tab, not System Alerts.
      showError(e instanceof Error ? e.message : `Failed to dismount ${letter}:.`, undefined, { kind: "notification" });
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
        />
      </Tooltip>

      <Tooltip content="Properties" position="top">
        <Button
          icon="info-sign"
          minimal
          small
          onClick={() => setPropertiesOpen(true)}
          className="vol-inline-btn"
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
