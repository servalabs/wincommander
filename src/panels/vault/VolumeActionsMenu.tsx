import { Button, Tooltip } from "@/components/ui/bp";
import { useState } from "react";
import useBackend from "../../hooks/useBackend";
import VolumePropertiesDialog from "./VolumePropertiesDialog";
import TierGate from "../../components/shared/TierGate";
import { showSuccess, showError } from "../../utils/toast";
import { vaultOperationError } from "@/lib/vaultOperationFeedback";
import VaultOperationNotice from "@/components/shared/VaultOperationNotice";
import './VolumeActionsMenu.css';

interface VolumeActionsMenuProps {
  letter: string;
  path: string | null;
  type: string;
  internalDrive?: number;
  accessible?: boolean;
  dismountAllowed?: boolean;
  dismountReason?: string | null;
  statusError?: string | null;
  onErrorChange?: (message: string) => void;
  onDismounted: () => void;
}

function VolumeActionsMenu({ letter, path, type, internalDrive, accessible = true, dismountAllowed, dismountReason, statusError, onDismounted, onErrorChange }: VolumeActionsMenuProps) {
  const { dismountVolume, getEncryptedVolumeStatus, openEncryptionVolume } = useBackend();

  const [dismounting, setDismounting] = useState(false);
  const [propertiesOpen, setPropertiesOpen] = useState(false);
  const [failure, setFailureMessage] = useState("");
  const setFailure = (message: string) => { setFailureMessage(message); onErrorChange?.(message ? `${driveLabel} ${message}` : ""); };
  const driveLabel = letter.endsWith(":") ? letter : `${letter}:`;
  const permissionHint = dismountAllowed === false && dismountReason ? vaultOperationError(`vault_${dismountReason}`, "dismount") : undefined;

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
      setFailure(verificationError);
      showError(verificationError, undefined, { kind: "notification" });
      return false;
    }
    onDismounted();
    showSuccess(`Volume ${driveLabel} ${forced ? "force-" : ""}dismounted.`);
    return true;
  };

  const handleDismount = async () => {
    if (statusError) { setFailure(statusError); return; }
    setDismounting(true);
    setFailure("");
    try {
      // An unavailable volume can belong to another or stale Windows sign-in
      // and may not return the normal "in use" failure that used to expose
      // the second force-dismount step. Dismount its exact engine slot now,
      // then keep the row until fresh status confirms it has disappeared.
      const result = await dismountVolume(letter, true, internalDrive);
      if (!result.success) {
        const message = vaultOperationError(result.error, "dismount");
        setFailure(message);
        showError(message, undefined, { kind: "notification" });
        return;
      }
      await completeDismount(true);
    } catch (e) {
      // Operational volume result → Notifications tab, not System Alerts.
      const message = vaultOperationError(e, "dismount");
      setFailure(message);
      showError(message, undefined, { kind: "notification" });
    } finally {
      setDismounting(false);
    }
  };

  const handleOpen = async () => {
    if (statusError) { setFailure(statusError); return; }
    setFailure("");
    try {
      const result = await openEncryptionVolume(letter);
      if (!result.success) throw new Error(result.error);
    } catch (error) { setFailure(vaultOperationError(error, "open")); }
  };

  return (
    <div className="vol-actions-group">
      <div className="flex items-center gap-1 flex-shrink-0">
      <Tooltip content="Open in Explorer" position="top">
        <Button
          icon="folder-open"
          minimal
          small
          onClick={handleOpen}
          disabled={!accessible || Boolean(statusError)}
          className="vol-inline-btn"
          aria-label={accessible ? `Open ${driveLabel} in Explorer` : `${driveLabel} is not accessible to this account`}
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
            disabled={Boolean(statusError)}
            onClick={handleDismount}
            className="vol-danger-btn"
            aria-label={`Force dismount ${driveLabel}`}
            aria-description={permissionHint}
          />
        </Tooltip>
      </TierGate>
      </div>
      {!onErrorChange && <VaultOperationNotice message={failure} />}

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
