import { Button, Icon, Dialog, FormGroup, InputGroup, Tooltip, CheckboxControl } from "@/components/ui/bp";
import { useState, useCallback, useEffect } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { DURATION_S, EASE } from "../../components/shared/motion";
import { staggerDelay } from "../../components/shared/AnimatedList";
import useBackend from "../../hooks/useBackend";
import type { EncryptionStatus, MountVolumeResult } from "../../hooks/useBackend";
import { useAppState } from "../../context/AppContext";
import { open } from "@tauri-apps/plugin-dialog";
import { useTheme } from "../../context/ThemeContext";
import CreateVolumeWizard from "./CreateVolumeWizard";
import VolumeActionsMenu from "./VolumeActionsMenu";
import SystemEncryptionSection from "./SystemEncryptionSection";
import RamDisksSection from "./RamDisksSection";
import StegoBackupSection from "./StegoBackupSection";
import { showSuccess, showError } from "../../utils/toast";
import type { EncryptionPartition } from "../../hooks/useBackend";
import PanelHeader from "../../components/shared/PanelHeader";
import TierGate from "../../components/shared/TierGate";
import SectionCard from "../../components/shared/SectionCard";
import useEntitlements from "../../hooks/useEntitlements";
import './index.css';
import DriveLetterPicker from "./DriveLetterPicker";

const validPim = (value: string) => !value || (Number.isInteger(Number(value)) && Number(value) >= 1 && Number(value) <= 2_147_468);
const MOUNT_ERROR_MAX_LENGTH = 300;

const boundedMountError = (error: unknown) => {
  const message = error instanceof Error ? error.message : "Failed to mount volume.";
  const normalized = message.replace(/\s+/g, " ").trim();
  if (normalized.includes("vault_engine_unlock_failed")) {
    return "WinCommander could not unlock this volume. Check that you selected the correct file and entered its original password, PIM, and keyfile.";
  }
  if (normalized.includes("vault_engine_drive_letter_unavailable")) {
    return "That drive letter is already in use. Choose a free drive letter, then try again.";
  }
  if (normalized.includes("vault_acl_apply_failed") || normalized.includes("vault_acl_readback_failed")) {
    return "WinCommander mounted the volume but could not verify its private Windows permissions, so it safely unmounted it. Use an NTFS-formatted container; FAT/exFAT volumes cannot carry private Windows permissions.";
  }
  if (normalized.includes("vault_not_authorized")) {
    return "This Windows account is not authorized to mount that container. Select the original container and use its original account, password, PIM, and keyfile.";
  }
  if (normalized.includes("vault_driver_unavailable")) {
    return "The encrypted-volume driver is unavailable. Open Settings and repair WinCommander, then retry.";
  }
  if (normalized.includes("vault_session_unavailable")) {
    return "WinCommander could not access your interactive Windows session. Sign out and back in, then retry.";
  }
  if (normalized.includes("vault_broker_failed")) {
    return "WinCommander's secure mount helper did not complete. The volume was not left mounted; check the driver status and retry after restarting WinCommander.";
  }
  return normalized.length > MOUNT_ERROR_MAX_LENGTH
    ? `${normalized.slice(0, MOUNT_ERROR_MAX_LENGTH - 1)}…`
    : normalized;
};

type VaultVolume = NonNullable<EncryptionStatus["volumes"]>[number];

function VaultPanel() {
  const { encryptionStatus, refreshVault, loading } = useAppState();

  const volumes = encryptionStatus?.volumes || [];

  return (
    <div className="vault-panel">
      <PanelHeader
        panelId="vault"
        title="Secure Storage"
        description="Create encrypted volumes and RAM disks to keep sensitive files locked away."
      />

      <div className="vault-storage-layout">
          <div className="vault-volumes-ramdisks-grid">
            <EncryptedVolumesTab
              volumes={volumes}
              refreshVault={refreshVault}
              initialLoading={loading.vault && encryptionStatus === null}
            />
            <RamDisksSection />
          </div>
          <StegoBackupSection />
      </div>
    </div>
  );
}

interface EncryptedVolumesTabProps {
  volumes: VaultVolume[];
  refreshVault: (silent?: boolean) => Promise<EncryptionStatus | null>;
  initialLoading: boolean;
}

// Encrypted Volumes tab: the card body (header, install-prompt, action row,
// volumes table) plus the Mount Encrypted Volume dialog — its state/handlers
// used to live on the whole panel; they now live here with the tab that
// owns them.
function EncryptedVolumesTab({ volumes, refreshVault, initialLoading }: EncryptedVolumesTabProps) {
  const { theme } = useTheme();
  const [mountDialogOpen, setMountDialogOpen] = useState(false);
  const [mountPath, setMountPath] = useState("");
  const [mountLetter, setMountLetter] = useState("Y");
  const [volumeKind, setVolumeKind] = useState<"standard" | "dual">("standard");
  const [volumeRole, setVolumeRole] = useState<"standard" | "outer" | "hidden">("standard");
  const [mountPassword, setMountPassword] = useState("");
  const [mountKeyfile, setMountKeyfile] = useState("");
  const [mountPim, setMountPim] = useState("");
  const [mountReadOnly, setMountReadOnly] = useState(false);
  const [mountRemovable, setMountRemovable] = useState(false);
  const [protectHidden, setProtectHidden] = useState(false);
  const [hiddenPassword, setHiddenPassword] = useState("");
  const [hiddenKeyfile, setHiddenKeyfile] = useState("");
  const [hiddenPim, setHiddenPim] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [createWizardOpen, setCreateWizardOpen] = useState(false);
  const [availableLetters, setAvailableLetters] = useState<string[]>([]);
  const [mountType, setMountType] = useState<'file' | 'partition'>('file');
  const [partitions, setPartitions] = useState<EncryptionPartition[]>([]);
  const [mountDetailsLoading, setMountDetailsLoading] = useState(false);
  const [mountedVolume, setMountedVolume] = useState<MountVolumeResult | null>(null);
  const [openingMountedVolume, setOpeningMountedVolume] = useState(false);
  const [mountFailure, setMountFailure] = useState("");

  const {
    mountVolume,
    verifyVaultDrive,
    openEncryptionVolume,
    getAvailableDriveLetters,
    getEncryptionPartitions,
    error
  } = useBackend();
  const { canUse } = useEntitlements();
  const accessibleVolumes = volumes.filter((volume) => volume.accessible !== false);
  const unavailableVolumes = volumes.filter((volume) => volume.accessible === false);

  const [mounting, setMounting] = useState(false);
  const isDualVolume = volumeKind === "dual";
  const isOuterDualVolume = isDualVolume && volumeRole === "outer";
  const requiresHiddenProtection = isOuterDualVolume && !mountReadOnly;
  const canMount = Boolean(
    mountPath
    && (mountPassword || mountKeyfile)
    && validPim(mountPim)
    && (!requiresHiddenProtection || (protectHidden && (hiddenPassword || hiddenKeyfile) && validPim(hiddenPim)))
    && !mounting
  );

  const resetMountForm = useCallback(() => {
    setMountFailure("");
    setMountPath("");
    setVolumeKind("standard");
    setVolumeRole("standard");
    setMountPassword("");
    setMountKeyfile("");
    setMountPim("");
    setMountReadOnly(false);
    setMountRemovable(false);
    setProtectHidden(false);
    setHiddenPassword("");
    setHiddenKeyfile("");
    setHiddenPim("");
    setMountLetter("Y");
    setMountType('file');
  }, []);

  const selectVolumeKind = useCallback((nextKind: "standard" | "dual") => {
    setVolumeKind(nextKind);
    setVolumeRole(nextKind === "dual" ? "outer" : "standard");
    setProtectHidden(false);
    setHiddenPassword("");
    setHiddenKeyfile("");
    setHiddenPim("");
  }, []);

  const selectVolumeRole = useCallback((nextRole: "outer" | "hidden") => {
    setVolumeRole(nextRole);
    setProtectHidden(false);
    setHiddenPassword("");
    setHiddenKeyfile("");
    setHiddenPim("");
  }, []);

  // Fetch available (unused) drive letters when the mount dialog opens
  const openMountDialog = useCallback(async () => {
    resetMountForm();
    setMountDialogOpen(true);
    setMountDetailsLoading(true);
    setPartitions([]);
    try {
      const [letterRes, partitionRes] = await Promise.all([
        getAvailableDriveLetters(),
        getEncryptionPartitions()
      ]);

      if (letterRes?.success && letterRes.data?.letters?.length) {
        setAvailableLetters(letterRes.data.letters);
        setMountLetter(letterRes.data.letters[0]);
      } else {
        const fallback = "DEFGHIJKLMNOPQRSTUVWXYZ".split("");
        setAvailableLetters(fallback);
        setMountLetter("Y");
      }

      if (partitionRes?.success && partitionRes.data?.partitions) {
        setPartitions(partitionRes.data.partitions);
        if (partitionRes.data.partitions.length > 0) {
          showSuccess(`Discovered ${partitionRes.data.partitions.length} mountable partitions.`);
        } else {
          showSuccess("No mountable partitions found. Showing file container mode.");
        }
      } else if (partitionRes && !partitionRes.success && partitionRes.error?.includes("No module found")) {
        // KT: This specifically catches when the Rust binary hasn't been restarted after command registration
        showError("Backend update required: Please restart the application to enable partition mounting.");
      } else if (partitionRes && !partitionRes.success) {
        showError(partitionRes.error || "Failed to fetch partitions.");
      }
    } catch (err) {
      console.error("Failed to fetch mount details", err);
      const fallback = "EFGHIJKLMNOPQRSTUVWXYZ".split("");
      setAvailableLetters(fallback);
    } finally {
      setMountDetailsLoading(false);
    }
  }, [resetMountForm, getAvailableDriveLetters, getEncryptionPartitions]);

  const handleBrowse = async () => {
    try {
      const selected = await open({
        multiple: false,
        filters: [{
          name: 'Container File',
          extensions: ['hc', 'tc', '*']
        }]
      });
      if (selected && typeof selected === 'string') {
        setMountPath(selected);
      }
    } catch (err) {
      console.error("Failed to open file picker", err);
    }
  };

  const handleBrowseKeyfile = async () => {
    try {
      const selected = await open({ multiple: false });
      if (selected && typeof selected === 'string') {
        setMountKeyfile(selected);
      }
    } catch (err) {
      console.error("Failed to open keyfile picker", err);
    }
  };

  const handleBrowseHiddenKeyfile = async () => {
    try {
      const selected = await open({ multiple: false });
      if (selected && typeof selected === 'string') setHiddenKeyfile(selected);
    } catch (err) {
      console.error("Failed to open hidden-volume keyfile picker", err);
    }
  };

  const handleMountVolume = useCallback(async () => {
    setMounting(true);
    setMountFailure("");
    try {
      const letters = await getAvailableDriveLetters();
      if (letters.success && letters.data && !letters.data.letters.includes(mountLetter)) {
        throw new Error(`Drive ${mountLetter}: is already in use. Dismount it first or choose a free drive letter.`);
      }
      const mountRequest = mountVolume({
        volumePath: mountPath,
        driveLetter: mountLetter,
        volumeKind,
        volumeRole,
        password: mountPassword,
        keyfiles: mountKeyfile ? [mountKeyfile] : [],
        pim: mountPim || undefined,
        readOnly: mountReadOnly,
        removable: mountRemovable,
        protectHidden: requiresHiddenProtection && protectHidden,
        hiddenPassword: requiresHiddenProtection ? hiddenPassword : undefined,
        hiddenKeyfiles: requiresHiddenProtection && hiddenKeyfile ? [hiddenKeyfile] : [],
        hiddenPim: requiresHiddenProtection ? hiddenPim || undefined : undefined,
        scope: "per-user",
        hardenAcl: true,
      });
      setMountPassword("");
      setHiddenPassword("");
      const result = await mountRequest;
      if (!result.success || !result.data) throw new Error(result.error || "Failed to mount volume");
      if (result.data.scope !== "per-user") {
        throw new Error("The encrypted volume was not mounted privately for this Windows account.");
      }
      await verifyVaultDrive(result.data.drive);
      const refreshed = await refreshVault(true);
      const isVisibleInThisSession = refreshed?.volumes?.some((volume) =>
        volume.letter === result.data?.drive
        && volume.internalDrive === result.data?.internalDrive,
      );
      if (!isVisibleInThisSession) {
        throw new Error("The encrypted volume was not available in this signed-in Windows session.");
      }
      setMountDialogOpen(false);
      resetMountForm();
      setMountedVolume(result.data);
    } catch (e) {
      // Operational volume result → Notifications tab, not System Alerts.
      const message = boundedMountError(e);
      setMountFailure(message);
      showError(message, undefined, { kind: "notification" });
    } finally {
      setMountPassword("");
      setHiddenPassword("");
      setMounting(false);
    }
  }, [getAvailableDriveLetters, hiddenKeyfile, hiddenPassword, hiddenPim, mountKeyfile, mountLetter, mountPassword, mountPim, mountPath, mountReadOnly, mountRemovable, mountVolume, protectHidden, refreshVault, requiresHiddenProtection, resetMountForm, verifyVaultDrive, volumeKind, volumeRole]);

  const handleOpenMountedVolume = useCallback(async () => {
    if (!mountedVolume) return;
    setOpeningMountedVolume(true);
    try {
      const result = await openEncryptionVolume(mountedVolume.drive);
      if (!result.success) throw new Error(result.error || "Could not open the encrypted volume.");
    } catch (error) {
      showError(error instanceof Error ? error.message : "Could not open the encrypted volume.");
    } finally {
      setOpeningMountedVolume(false);
    }
  }, [mountedVolume, openEncryptionVolume]);

  const handleMountDialogKeyDown = useCallback((event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "Enter" || event.shiftKey || event.defaultPrevented) return;

    const target = event.target as HTMLElement | null;
    if (!target) return;
    if (target.closest("button")) return;
    if (target instanceof HTMLTextAreaElement) return;
    if (!canMount) return;

    event.preventDefault();
    void handleMountVolume();
  }, [canMount, handleMountVolume]);

  const handleMountFieldEnter = useCallback((event: React.KeyboardEvent<HTMLElement>) => {
    if (event.key !== "Enter" || event.shiftKey || event.defaultPrevented) return;
    if (!canMount) return;
    event.preventDefault();
    void handleMountVolume();
  }, [canMount, handleMountVolume]);

  useEffect(() => {
    if (error) showError(error);
  }, [error]);

  return (
    <>
      <SectionCard
        title="Encrypted Volumes"
        icon="lock"
        headerRight={
          <div className="flex items-center gap-2">
            {accessibleVolumes.length > 0 && (
              <span className="vault-status-badge vault-status-badge--mounted">
                <i />{accessibleVolumes.length} mounted
              </span>
            )}
            {unavailableVolumes.length > 0 && (
              <span className="vault-status-badge vault-status-badge--unavailable">
                {unavailableVolumes.length} needs attention
              </span>
            )}
            <SystemEncryptionSection />
            <Tooltip content="Refresh status" position="top">
              <Button
                icon="refresh"
                minimal
                small
                onClick={() => refreshVault(true)}
                aria-label="Refresh encryption volume status"
              />
            </Tooltip>
          </div>
        }
      >
        {!canUse("paid") && (
          <div className="vault-card install-prompt">
            <Icon icon="lock" size={48} className="install-icon" />
            <h2>Encrypted Volumes is a Pro Feature</h2>
            <p>Unlock WinCommander Pro to create, mount and dismount encrypted volumes.</p>
          </div>
        )}

        {/* Keep the storage actions above the mounted-volumes area so
            create/mount controls stay reachable before the empty/table
            body claims the card height. */}
        <div className="vault-card-action-row">
          <TierGate tier="paid" featureLabel="Encrypted volumes">
            <Button
              icon="add"
              text="Create Volume"
              intent="primary"
              onClick={() => setCreateWizardOpen(true)}
              className="vault-action-btn vault-action-btn--primary"
            />
            <Button
              minimal
              icon="folder-open"
              text="Mount Volume"
              onClick={() => openMountDialog()}
              className="vault-action-btn"
            />
          </TierGate>
        </div>

        <div className={`vault-content ${volumes.length === 0 ? "vault-content--empty" : ""}`}>
          {initialLoading ? (
            <div className="empty-state" role="status" aria-busy="true">
              <Icon icon="refresh" className="empty-icon" size={32} />
              <p>Checking encrypted volumes…</p>
            </div>
          ) : volumes.length > 0 ? (
            <table className="volumes-table wc-table wc-table--striped">
              <thead>
                <tr>
                  <th>DRIVE</th>
                  <th>TYPE</th>
                  <th>PATH</th>
                  <th></th>
                </tr>
              </thead>
              {/* AnimatePresence lets newly-mounted volumes fade in with a
                  staggered delay, and lets dismounted rows exit before DOM
                  removal. No celebration/success flourish — enter/exit only.
                  staggerDelay caps per-row delay so long lists never animate
                  over seconds. motion.tr uses opacity only — no width/height
                  reflow. MotionConfig in App.tsx handles reduced-motion. */}
              <tbody>
                <AnimatePresence initial={false}>
                  {volumes.map((vol: any, idx: number) => (
                    <motion.tr
                      key={vol.letter}
                      initial={{ opacity: 0 }}
                      animate={{ opacity: 1 }}
                      exit={{ opacity: 0 }}
                      transition={{
                        delay: staggerDelay(idx),
                        duration: DURATION_S.fast,
                        ease: EASE.enter,
                      }}
                    >
                      <td className="mono-cell">
                        {/* Active-mount indicator — pulsing green dot makes
                            live volumes obvious at a glance, matching the
                            status-badge styling above. */}
                        <span className={`vault-active-dot${vol.accessible === false ? " vault-active-dot--unavailable" : ""}`} aria-hidden />
                        <strong>{vol.letter}</strong>
                      </td>
                      <td><span className={`type-badge${vol.type === "Hidden" ? " type-badge--hidden" : ""}`}>{vol.accessible === false ? "Unavailable" : vol.type}</span></td>
                      <td className="path-cell">
                        <span className="truncate-path" title={vol.path}>{vol.path}</span>
                        {vol.accessible === false && <span className="vault-volume-unavailable">Not available in this Windows sign-in. Dismount, then mount it again.</span>}
                      </td>
                      <td className="actions-cell">
                        <VolumeActionsMenu
                          letter={vol.letter}
                          path={vol.path}
                          type={vol.type}
                          internalDrive={vol.internalDrive}
                          accessible={vol.accessible !== false}
                          onDismounted={() => refreshVault(true)}
                        />
                      </td>
                    </motion.tr>
                  ))}
                </AnimatePresence>
              </tbody>
            </table>
          ) : (
            <div className="empty-state">
              <Icon icon="clean" className="empty-icon" size={32} />
              <p>No encrypted volumes mounted</p>
            </div>
          )}
        </div>
      </SectionCard>

      <CreateVolumeWizard
        isOpen={createWizardOpen}
        onClose={() => setCreateWizardOpen(false)}
        onCreated={() => { setCreateWizardOpen(false); refreshVault(true); }}
      />
      <Dialog
        isOpen={mountDialogOpen}
        onClose={() => {
          setMountDialogOpen(false);
          resetMountForm();
        }}
        title="Mount Encrypted Volume"
        className={`mount-dialog ${theme === 'light' ? 'light' : ''}`}
        backdropProps={{
          style: {
            backgroundColor: theme === 'light' ? 'rgba(0, 0, 0, 0.4)' : 'rgba(0, 0, 0, 0.7)',
            backdropFilter: 'blur(4px)'
          }
        }}
      >
        <div className="wc-dialog-body" onKeyDown={handleMountDialogKeyDown}>
          {mountFailure && (
            <div className="mount-error" role="alert">
              <Icon icon="warning-sign" size={16} />
              <span>{mountFailure}</span>
            </div>
          )}
          {mounting && (mountPim || (protectHidden && hiddenPim)) && (
            <div className="mount-progress" role="status">
              <Icon icon="time" size={16} />
              <span>Unlocking with your PIM can take several minutes. Your password was cleared for safety.</span>
            </div>
          )}
          <div className="mount-volume-selector" role="radiogroup" aria-label="Volume kind">
            <button
              type="button"
              role="radio"
              aria-checked={volumeKind === "standard"}
              className={`mount-volume-selector__option${volumeKind === "standard" ? " is-active" : ""}`}
              onClick={() => selectVolumeKind("standard")}
            >
              <Icon icon="lock" size={14} />
              <span>Standard volume</span>
            </button>
            <button
              type="button"
              role="radio"
              aria-checked={volumeKind === "dual"}
              className={`mount-volume-selector__option${volumeKind === "dual" ? " is-active" : ""}`}
              onClick={() => selectVolumeKind("dual")}
            >
              <Icon icon="layers" size={14} />
              <span>Hidden + decoy</span>
            </button>
          </div>

          {isDualVolume && (
            <div className="mount-volume-selector mount-volume-selector--role" role="radiogroup" aria-label="Dual-volume action">
              <button
                type="button"
                role="radio"
                aria-checked={volumeRole === "hidden"}
                className={`mount-volume-selector__option${volumeRole === "hidden" ? " is-active" : ""}`}
                onClick={() => selectVolumeRole("hidden")}
              >
                <Icon icon="eye-off" size={14} />
                <span>Open hidden volume</span>
              </button>
              <button
                type="button"
                role="radio"
                aria-checked={volumeRole === "outer"}
                className={`mount-volume-selector__option${volumeRole === "outer" ? " is-active" : ""}`}
                onClick={() => selectVolumeRole("outer")}
              >
                <Icon icon="eye-open" size={14} />
                <span>Open visible decoy</span>
              </button>
            </div>
          )}
          {/* Mount source toggle. The previous Button-with-active pattern
              didn't stand out clearly — the user couldn't tell at a glance
              which mode was selected. Switched to a segmented control where
              the active option carries the accent border + tinted background
              + a green active dot. */}
          <div className="mount-type-selector mb-4" role="tablist" aria-label="Mount source">
            <button
              type="button"
              role="tab"
              aria-selected={mountType === 'file'}
              className={`mount-type-btn${mountType === 'file' ? ' is-active' : ''}`}
              onClick={() => setMountType('file')}
            >
              {mountType === 'file' && <span className="vault-active-dot" aria-hidden />}
              <Icon icon="document" size={14} />
              <span>File Container</span>
            </button>
            <button
              type="button"
              role="tab"
              aria-selected={mountType === 'partition'}
              className={`mount-type-btn${mountType === 'partition' ? ' is-active' : ''}`}
              onClick={() => setMountType('partition')}
            >
              {mountType === 'partition' && <span className="vault-active-dot" aria-hidden />}
              <Icon icon="database" size={14} />
              <span>Partition / Drive</span>
            </button>
          </div>

          {mountType === 'file' ? (
            <FormGroup label="Volume Path" labelFor="volume-path">
              <InputGroup
                id="volume-path"
                placeholder="/path/to/volume.hc"
                value={mountPath}
                autoComplete="off"
                onChange={(e) => setMountPath(e.target.value)}
                onKeyDown={handleMountFieldEnter}
                rightElement={
                  <Button icon="folder-open" minimal aria-label="Browse for an encrypted volume" onClick={handleBrowse} />
                }
              />
            </FormGroup>
          ) : (
            <FormGroup label="Select Partition" labelFor="partition-select">
              {/* List of partitions as selectable rows. Was a one-line
                  HTMLSelect before — the long labels truncated and hid the
                  "active" / drive-letter signal so the user couldn't tell
                  which partition was already mounted. Each row now shows
                  the partition's model + size + disk/part numbers, plus a
                  green active-dot and bright drive-letter chip when the OS
                  has it mounted. The selected row gets an accent border. */}
              <div className="partition-list" role="listbox" id="partition-select" aria-label="Select partition">
                {mountDetailsLoading ? (
                  <div className="partition-list-empty" role="status" aria-busy="true">Discovering partitions…</div>
                ) : partitions.length === 0 ? (
                  <div className="partition-list-empty">No mountable partitions found.</div>
                ) : (
                  partitions.map((p) => {
                    const isActive = !!p.driveLetter;
                    const isSelected = mountPath === p.devicePath;
                    return (
                      <button
                        type="button"
                        key={p.devicePath}
                        role="option"
                        aria-selected={isSelected}
                        className={`partition-row${isSelected ? " is-selected" : ""}${isActive ? " is-active" : ""}`}
                        onClick={() => setMountPath(p.devicePath)}
                      >
                        <span className="partition-row-lead">
                          {isActive ? (
                            <span className="vault-active-dot" aria-hidden />
                          ) : (
                            <span className="partition-row-dot-spacer" aria-hidden />
                          )}
                        </span>
                        <span className="partition-row-main">
                          <span className="partition-row-title">
                            {p.model}
                            <span className="partition-row-size">{p.size}</span>
                          </span>
                          <span className="partition-row-sub">
                            Disk {p.diskNumber} · Part {p.partitionNumber}
                            {p.busType ? ` · ${p.busType}` : ""}
                          </span>
                        </span>
                        {isActive && (
                          <span className="partition-row-letter" title={`Currently mounted at ${p.driveLetter}:\\`}>
                            {p.driveLetter}:
                          </span>
                        )}
                      </button>
                    );
                  })
                )}
              </div>
            </FormGroup>
          )}

          <FormGroup label="Drive Letter" labelFor="drive-letter">
            <DriveLetterPicker
              id="drive-letter"
              value={mountLetter}
              onChange={setMountLetter}
              onKeyDown={handleMountFieldEnter}
              letters={availableLetters}
            />
          </FormGroup>

          <FormGroup
            label={volumeRole === "hidden" ? "Hidden volume password" : "Password"}
            labelFor="password"
            helperText={volumeRole === "hidden" ? "Use the hidden volume's password." : "Make sure CAPS Lock is off."}
          >
            <InputGroup
              id="password"
              type={showPassword ? "text" : "password"}
              value={mountPassword}
              autoComplete="off"
              onChange={(e) => setMountPassword(e.target.value)}
              onKeyDown={handleMountFieldEnter}
              rightElement={
                <Button
                  icon={showPassword ? "eye-off" : "eye-open"}
                  minimal
                  aria-label={showPassword ? "Hide volume password" : "Show volume password"}
                  onClick={() => setShowPassword(!showPassword)}
                />
              }
            />
          </FormGroup>

          <FormGroup label={volumeRole === "hidden" ? "Hidden volume keyfile (optional)" : "Keyfile (optional)"} labelFor="mount-keyfile">
            <InputGroup
              id="mount-keyfile"
              placeholder="Path to keyfile or folder"
              value={mountKeyfile}
              autoComplete="off"
              onChange={(e) => setMountKeyfile(e.target.value)}
              onKeyDown={handleMountFieldEnter}
              rightElement={
                <Button icon="folder-open" minimal aria-label="Browse for a volume keyfile" onClick={handleBrowseKeyfile} />
              }
            />
          </FormGroup>

          <FormGroup label={volumeRole === "hidden" ? "Hidden volume PIM (optional)" : "PIM (optional)"} labelFor="mount-pim" helperText="Leave blank for default; otherwise it must match the volume's creation PIM.">
            <InputGroup
              id="mount-pim"
              type="number"
              min={1}
              max={2147468}
              placeholder="Default"
              value={mountPim}
              autoComplete="off"
              onChange={(e) => setMountPim(e.target.value)}
              onKeyDown={handleMountFieldEnter}
            />
          </FormGroup>

          <div className="mount-options-grid" aria-label="Mount options">
            <label className="quick-toggle">
              <CheckboxControl
                checked={mountReadOnly}
                ariaLabel="Mount read-only"
                onChange={event => {
                  const checked = event.currentTarget.checked;
                  setMountReadOnly(checked);
                  if (checked) setProtectHidden(false);
                }}
              />
              <span>Read-only</span>
              <span className="quick-desc">Blocks every write to this volume.</span>
            </label>
            <label className="quick-toggle">
              <CheckboxControl
                checked={mountRemovable}
                ariaLabel="Mount as removable media"
                onChange={event => setMountRemovable(event.currentTarget.checked)}
              />
              <span>Removable media</span>
              <span className="quick-desc">Reports the mounted volume as removable.</span>
            </label>
            {isOuterDualVolume && (
              <label className="quick-toggle">
                <CheckboxControl
                  checked={protectHidden}
                  disabled={mountReadOnly}
                  ariaLabel="Protect hidden volume"
                  onChange={event => setProtectHidden(event.currentTarget.checked)}
                />
                <span>Protect hidden volume</span>
                <span className="quick-desc">Required for writable outer-decoy mounts.</span>
              </label>
            )}
            <div className="quick-toggle quick-toggle--locked" aria-label="Private NTFS permissions enabled">
              <Icon icon="lock" size={14} />
              <span>Private NTFS permissions</span>
              <span className="quick-desc">Always enabled for this user-only encrypted drive.</span>
            </div>
          </div>

          {requiresHiddenProtection && protectHidden && (
            <div className="mount-hidden-protection">
              <FormGroup label="Hidden volume password or keyfile" labelFor="hidden-password" helperText="Used only in memory to protect the hidden region; it is not mounted.">
                <InputGroup
                  id="hidden-password"
                  type={showPassword ? "text" : "password"}
                  value={hiddenPassword}
                  autoComplete="off"
                  onChange={event => setHiddenPassword(event.target.value)}
                  onKeyDown={handleMountFieldEnter}
                />
              </FormGroup>
              <FormGroup label="Hidden keyfile (optional)" labelFor="hidden-keyfile">
                <InputGroup
                  id="hidden-keyfile"
                  value={hiddenKeyfile}
                  autoComplete="off"
                  onChange={event => setHiddenKeyfile(event.target.value)}
                  rightElement={<Button icon="folder-open" minimal aria-label="Browse for hidden-volume keyfile" onClick={handleBrowseHiddenKeyfile} />}
                />
              </FormGroup>
              <FormGroup label="Hidden PIM (optional)" labelFor="hidden-pim">
                <InputGroup
                  id="hidden-pim"
                  type="number"
                  min={1}
                  max={2147468}
                  placeholder="Default"
                  value={hiddenPim}
                  autoComplete="off"
                  onChange={event => setHiddenPim(event.target.value)}
                  onKeyDown={handleMountFieldEnter}
                />
              </FormGroup>
            </div>
          )}
        </div>

        <div className="mount-dialog-footer">
          <Button
            icon="cross"
            text="CANCEL"
            onClick={() => setMountDialogOpen(false)}
            minimal
            className="modal-cancel-btn"
          />
          <Button
            icon="unlock"
            text="MOUNT VOLUME"
            onClick={handleMountVolume}
            loading={mounting}
            disabled={!canMount}
            className="modal-primary-btn"
          />
        </div>
      </Dialog>

      <Dialog
        isOpen={mountedVolume !== null}
        onClose={() => setMountedVolume(null)}
        title="Encrypted volume mounted"
        className={`mount-result-dialog ${theme === 'light' ? 'light' : ''}`}
      >
        <div className="mount-result-dialog__body">
          <Icon icon="warning-sign" size={28} className="mount-result-dialog__icon" />
          <div>
            <strong>{mountedVolume?.drive} is ready in File Explorer</strong>
            <p>It is visible only to this signed-in Windows account and has private NTFS permissions.</p>
            <p className="mount-result-dialog__note">Dismount it when you are finished. Windows administrators can still manage this computer.</p>
          </div>
        </div>
        <div className="mount-dialog-footer">
          <Button minimal text="DONE" onClick={() => setMountedVolume(null)} className="modal-cancel-btn" />
          <Button
            icon="folder-open"
            text="OPEN IN FILE EXPLORER"
            onClick={() => void handleOpenMountedVolume()}
            loading={openingMountedVolume}
            className="modal-primary-btn"
          />
        </div>
      </Dialog>
    </>
  );
}

export default VaultPanel;
