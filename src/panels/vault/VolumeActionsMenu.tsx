import { Button, Dialog, DialogBody, DialogFooter, Tooltip } from "@/components/ui/bp";
import { open } from "@tauri-apps/plugin-shell";
import { useState } from "react";
import useBackend from "../../hooks/useBackend";
import VolumePropertiesDialog from "./VolumePropertiesDialog";
import TierGate from "../../components/shared/TierGate";
import { showSuccess, showError } from "../../utils/toast";
import { notifyVaultSyncRecovery } from "@/lib/vaultSyncWarning";
import { isSyncthingSetupUrl } from "@/lib/vaultSyncRecovery";
import { vaultOperationError } from "@/lib/vaultOperationFeedback";
import { personalVaultSyncError, personalVaultSyncSetupMessage, validatePersonalVaultSyncFolders } from "@/lib/personalVaultSyncFeedback";
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
  const { dismountVolume, getEncryptedVolumeStatus, openEncryptionVolume, enablePersonalVaultSync } = useBackend();

  const [dismounting, setDismounting] = useState(false);
  const [enrollingSync, setEnrollingSync] = useState(false);
  const [syncSetupOpen, setSyncSetupOpen] = useState(false);
  const [syncFolders, setSyncFolders] = useState(["Phone\\Camera"]);
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

  const handleEnablePersonalSync = async () => {
    if (internalDrive === undefined) return;
    const validated = validatePersonalVaultSyncFolders(syncFolders);
    if (enrollingSync) return;
    if (!validated.ok) {
      setFailure(validated.message);
      return;
    }
    setEnrollingSync(true);
    setFailure("");
    try {
      let guiUrl = "";
      let pairingRequired = false;
      for (const relativePath of validated.folders) {
        const enrollment = await enablePersonalVaultSync(internalDrive, relativePath);
        if (enrollment.recovery_required) {
          setSyncSetupOpen(false);
          window.setTimeout(() => notifyVaultSyncRecovery(driveLabel, internalDrive, enrollment), 350);
          return;
        }
        if (!enrollment.enabled) throw new Error("vault_broker_rejected");
        pairingRequired ||= enrollment.pairing_required === true;
        if (isSyncthingSetupUrl(enrollment.gui_url)) guiUrl = enrollment.gui_url;
      }
      setSyncSetupOpen(false);
      const configured = validated.folders.map(relativePath => `${driveLabel}\\${relativePath}`).join(", ");
      const message = personalVaultSyncSetupMessage(configured, pairingRequired);
      showSuccess(message);
      try {
        if (!guiUrl) throw new Error("invalid_gui_url");
        await open(guiUrl);
      } catch {
        showError("Sync is configured, but its setup page could not open. Use Open Syncthing from this Vault after setup.", undefined, { kind: "notification" });
      }
    } catch (error) {
      const message = personalVaultSyncError(error);
      setFailure(message);
      showError(message, undefined, { kind: "notification" });
    } finally {
      setEnrollingSync(false);
    }
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
        <Tooltip content="Enable sync for a folder in this personal Vault" position="top">
          <Button
            icon="cloud-upload"
            minimal
            small
            loading={enrollingSync}
            disabled={enrollingSync || !accessible || Boolean(statusError) || internalDrive === undefined}
            onClick={() => { setFailure(""); setSyncSetupOpen(true); }}
            className="vol-inline-btn"
            aria-label={`Enable Syncthing for a folder in ${driveLabel}`}
          />
        </Tooltip>
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

      <Dialog isOpen={syncSetupOpen} onClose={() => { if (!enrollingSync) setSyncSetupOpen(false); }}
        title={`Set up sync for ${driveLabel}`} style={{ width: 560 }} canEscapeKeyClose={!enrollingSync}
        canOutsideClickClose={!enrollingSync} isCloseButtonShown={!enrollingSync}>
        <DialogBody>
          <p>Choose one or more separate folders inside this personal Vault. WinCommander will install Syncthing for your Windows account if needed.</p>
          <p>Each entry is a separate phone-sync folder. They cannot overlap: <strong>Phone\Camera</strong> and <strong>Phone\Documents</strong> are safe; <strong>Phone</strong> and <strong>Phone\Camera</strong> are not.</p>
          <p>Pausing a folder does not stop Syncthing or pause your other folders. When this Vault is dismounted, its configured folders pause individually and resume after remount.</p>
          <div className="space-y-2">
            {syncFolders.map((folder, index) => <div key={index} className="flex gap-2">
              <div className="min-w-0 flex-1">
                <label className="sr-only" htmlFor={`sync-folder-${internalDrive}-${index}`}>Sync folder {index + 1} inside the Vault</label>
                <input id={`sync-folder-${internalDrive}-${index}`} value={folder} maxLength={240}
                  onChange={event => setSyncFolders(current => current.map((entry, entryIndex) => entryIndex === index ? event.target.value : entry))} disabled={enrollingSync}
                  className="w-full rounded-md border p-2" placeholder="Phone\Camera" />
              </div>
              {syncFolders.length > 1 && <Button minimal disabled={enrollingSync} aria-label={`Remove sync folder ${index + 1}`}
                onClick={() => setSyncFolders(current => current.filter((_, entryIndex) => entryIndex !== index))}>Remove</Button>}
            </div>)}
          </div>
          <Button minimal disabled={enrollingSync || syncFolders.length >= 32} onClick={() => setSyncFolders(current => [...current, ""])}>Add another folder</Button>
          {enrollingSync && <p role="status" aria-live="polite">Setting up Syncthing… The first setup may need to download it. Keep this Vault mounted.</p>}
          <VaultOperationNotice message={failure} />
        </DialogBody>
        <DialogFooter actions={<>
          <Button onClick={() => setSyncSetupOpen(false)} disabled={enrollingSync}>Cancel</Button>
          <Button intent="primary" onClick={handleEnablePersonalSync} loading={enrollingSync}
            disabled={enrollingSync || !syncFolders.some(folder => folder.trim())}>Enable sync folders</Button>
        </>} />
      </Dialog>

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
