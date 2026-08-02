import { Button, Icon, Dialog, FormGroup, InputGroup, HTMLSelect, Tooltip } from "@/components/ui/bp";
import { useState, useCallback, useEffect } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { DURATION_S, EASE } from "../../components/shared/motion";
import { staggerDelay } from "../../components/shared/AnimatedList";
import useBackend from "../../hooks/useBackend";
import type { EncryptionStatus } from "../../hooks/useBackend";
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
  refreshVault: (silent?: boolean) => Promise<void>;
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
  const [mountPassword, setMountPassword] = useState("");
  const [mountKeyfile, setMountKeyfile] = useState("");
  const [mountPim, setMountPim] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [createWizardOpen, setCreateWizardOpen] = useState(false);
  const [availableLetters, setAvailableLetters] = useState<string[]>([]);
  const [mountType, setMountType] = useState<'file' | 'partition'>('file');
  const [partitions, setPartitions] = useState<EncryptionPartition[]>([]);
  const [mountDetailsLoading, setMountDetailsLoading] = useState(false);

  const {
    mountVolume,
    getAvailableDriveLetters,
    getEncryptionPartitions,
    error
  } = useBackend();
  const { canUse } = useEntitlements();

  const [mounting, setMounting] = useState(false);

  const resetMountForm = useCallback(() => {
    setMountPath("");
    setMountPassword("");
    setMountKeyfile("");
    setMountPim("");
    setMountLetter("Y");
    setMountType('file');
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

  const handleMountVolume = useCallback(async () => {
    setMounting(true);
    try {
      await mountVolume(mountPath, mountLetter, mountPassword, mountKeyfile || undefined, mountPim || undefined);
      setMountDialogOpen(false);
      resetMountForm();
      await refreshVault(true);
      showSuccess(`Volume mounted at ${mountLetter}:.`);
    } catch (e) {
      // Operational volume result → Notifications tab, not System Alerts.
      showError(e instanceof Error ? e.message : "Failed to mount volume.", undefined, { kind: "notification" });
      setMountPassword("");
    } finally {
      setMounting(false);
    }
  }, [mountKeyfile, mountLetter, mountPassword, mountPim, mountPath, mountVolume, refreshVault, resetMountForm]);

  const handleMountDialogKeyDown = useCallback((event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "Enter" || event.shiftKey || event.defaultPrevented) return;

    const target = event.target as HTMLElement | null;
    if (!target) return;
    if (target.closest("button")) return;
    if (target instanceof HTMLTextAreaElement) return;
    if (!mountPath || (!mountPassword && !mountKeyfile) || mounting) return;

    event.preventDefault();
    void handleMountVolume();
  }, [handleMountVolume, mountKeyfile, mountPassword, mountPath, mounting]);

  const handleMountFieldEnter = useCallback((event: React.KeyboardEvent<HTMLElement>) => {
    if (event.key !== "Enter" || event.shiftKey || event.defaultPrevented) return;
    if (!mountPath || (!mountPassword && !mountKeyfile) || mounting) return;
    event.preventDefault();
    void handleMountVolume();
  }, [handleMountVolume, mountKeyfile, mountPassword, mountPath, mounting]);

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
            {volumes.length > 0 && (
              <span className="vault-status-badge vault-status-badge--mounted">
                <i />{volumes.length} mounted
              </span>
            )}
            <SystemEncryptionSection compact />
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
                        <span className="vault-active-dot" aria-hidden />
                        <strong>{vol.letter}</strong>
                      </td>
                      <td><span className={`type-badge${vol.type === "Hidden" ? " type-badge--hidden" : ""}`}>{vol.type}</span></td>
                      <td className="path-cell">
                        <span className="truncate-path" title={vol.path}>{vol.path}</span>
                      </td>
                      <td className="actions-cell">
                        <VolumeActionsMenu
                          letter={vol.letter}
                          path={vol.path}
                          type={vol.type}
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
            <HTMLSelect
              id="drive-letter"
              value={mountLetter}
              onChange={e => setMountLetter(e.target.value)}
              onKeyDown={handleMountFieldEnter}
              options={availableLetters.length
                ? availableLetters.map(l => ({ value: l, label: `${l}:\\` }))
                : "EFGHIJKLMNOPQRSTUVWXYZ".split("").map(l => ({ value: l, label: `${l}:\\` }))
              }
              fill
            />
          </FormGroup>

          <FormGroup
            label="Password"
            labelFor="password"
            helperText="Make sure CAPS Lock is off."
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

          <FormGroup label="Keyfile (optional)" labelFor="mount-keyfile">
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

          <FormGroup label="PIM (optional)" labelFor="mount-pim" helperText="Leave blank for default. PIM < 485 requires a password ≥ 20 chars.">
            <InputGroup
              id="mount-pim"
              type="number"
              min={1}
              placeholder="Default"
              value={mountPim}
              autoComplete="off"
              onChange={(e) => setMountPim(e.target.value)}
              onKeyDown={handleMountFieldEnter}
            />
          </FormGroup>
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
            disabled={!mountPath || (!mountPassword && !mountKeyfile) || mounting}
            className="modal-primary-btn"
          />
        </div>
      </Dialog>
    </>
  );
}

export default VaultPanel;
