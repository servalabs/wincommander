// src/panels/privacy/CryptoEraseTargetsSection.tsx
//
// Target pickers for the `bitlocker_erase` and `veracrypt_header_destroy`
// lockdown steps (group privacyClean). Both steps previously had no way to
// choose a target at all: bitlocker_erase silently defaulted to the OS
// drive, veracrypt_header_destroy had no target and always failed. This
// lets the operator pick exactly which encrypted volumes get crypto-erased
// when the cascade fires (sidebar button, hotkey, distress phrase, dead-man's
// switch, or the Calculator gate's destroy PIN).
//
// Mirrors RemoveUsersSection.tsx (BitLocker checklist) and the "Folders to
// Delete on Lockdown" block (VeraCrypt path list) for visual rhythm — same
// sd-row / sd-shred-folders CSS families, no new stylesheet needed.
//
// Settings patch semantics: the patch endpoint deep-merges objects but
// REPLACES arrays wholesale, so every selection change sends the full
// desired array (see cryptoEraseCascadeUtils.ts) — never a delta.
//
// No separate typed nuclear-ack here (unlike the manual Crypto-Erase
// picker's OS-volume ceremony): selecting the system drive means it WILL
// be targeted the next time the cascade fires. The trigger itself (destroy
// PIN, distress phrase, etc.) is the confirmation — this panel is where
// that intent is configured ahead of time, not where it's fired.

import { useCallback, useEffect, useState } from "react";
import { Button, Callout, Checkbox, Icon, Popover, Spinner, Tooltip } from "@/components/ui/bp";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import useBackend, { type BitLockerVolume, type EncryptionPartition } from "../../hooks/useBackend";
import type { VeraCryptDeviceEraseTarget } from "../../types/settings";
import EmptyState from "../../components/shared/EmptyState";
import { escrowRiskOf } from "../../lib/cryptoEraseTargets";
import { showError } from "../../utils/toast";
import {
  toggleBitlockerDrive,
  isBitlockerDriveSelected,
  addVeracryptPath,
  removeVeracryptPath,
} from "./cryptoEraseCascadeUtils";
import "./LockdownConfigSection.css";
import "./CryptoEraseTargetsSection.css";

const SYSTEM_TIP =
  "The volume Windows boots from. If the cascade erases its keys this machine will not start again — and unlike the manual Crypto-Erase picker there is no confirmation at trigger time.";
const ESCROW_TIP =
  "A recovery key for this volume is backed up to Entra/Active Directory or a Microsoft account. That copy survives the erase, so the volume stays recoverable until you delete the saved key there as well.";

function HowItWorks() {
  return (
    <Popover
      position="bottom-end"
      content={
        <div className="ce-cascade-explainer">
          <strong>What gets destroyed</strong>
          <p>
            Crypto-erase destroys the key that makes an encrypted volume readable, not the data
            itself. It takes seconds instead of hours and works on SSDs, where overwriting is
            unreliable.
          </p>
          <strong>When it fires</strong>
          <p>
            Only when the cascade runs — the sidebar button, the hotkey, a distress phrase, the
            dead-man&rsquo;s switch, or the Calculator gate&rsquo;s destroy PIN — and only if the
            matching destruct step above is enabled.
          </p>
          <strong>No second confirmation</strong>
          <p>
            Selecting a volume here IS the confirmation. The trigger fires immediately with no
            further prompt, which is the point of a panic control.
          </p>
        </div>
      }
    >
      <Button className="sd-bulk-btn" minimal small icon="info-sign" text="How it works" />
    </Popover>
  );
}

interface Props {
  bitlockerDrives: string[];
  veracryptPaths: string[];
  veracryptDevices: VeraCryptDeviceEraseTarget[];
  onPatch: (patch: {
    cryptoEraseBitlockerDrives?: string[];
    cryptoEraseVeracryptPaths?: string[];
    cryptoEraseVeracryptDevices?: VeraCryptDeviceEraseTarget[];
  }) => void;
}

export default function CryptoEraseTargetsSection({
  bitlockerDrives,
  veracryptPaths,
  veracryptDevices,
  onPatch,
}: Props) {
  const { getBitLockerVolumes, getEncryptionPartitions } = useBackend();
  const [volumes, setVolumes] = useState<BitLockerVolume[]>([]);
  const [partitions, setPartitions] = useState<EncryptionPartition[]>([]);
  const [loading, setLoading] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    const [res, partitionRes] = await Promise.all([
      getBitLockerVolumes(),
      getEncryptionPartitions(),
    ]);
    setLoading(false);
    if (res.success && Array.isArray(res.data)) {
      setVolumes(res.data);
    } else if (!res.success) {
      showError(res.error || "Failed to load BitLocker volumes");
    }
    if (partitionRes.success && Array.isArray(partitionRes.data?.partitions)) {
      setPartitions(
        partitionRes.data.partitions.filter((partition) => partition.safeForCreation),
      );
    } else if (!partitionRes.success) {
      showError(partitionRes.error || "Failed to load candidate VeraCrypt partitions");
    }
  }, [getBitLockerVolumes, getEncryptionPartitions]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const handleToggleDrive = useCallback(
    (drive: string) => {
      onPatch({ cryptoEraseBitlockerDrives: toggleBitlockerDrive(bitlockerDrives, drive) });
    },
    [bitlockerDrives, onPatch],
  );

  const addContainer = useCallback(async () => {
    try {
      const picked = await openDialog({
        multiple: false,
        filters: [{ name: "Container File", extensions: ["hc", "tc", "*"] }],
        title: "Pick a VeraCrypt container to crypto-erase on lockdown",
      });
      if (typeof picked !== "string") return;
      const next = addVeracryptPath(veracryptPaths, picked);
      if (next === veracryptPaths) {
        showError("Container is already in the erase list.");
        return;
      }
      onPatch({ cryptoEraseVeracryptPaths: next });
    } catch (err) {
      showError(`Couldn't add container: ${err}`);
    }
  }, [veracryptPaths, onPatch]);

  const removeContainer = useCallback(
    (path: string) => {
      onPatch({ cryptoEraseVeracryptPaths: removeVeracryptPath(veracryptPaths, path) });
    },
    [veracryptPaths, onPatch],
  );

  const togglePartition = useCallback(
    (partition: EncryptionPartition) => {
      const selected = veracryptDevices.some(
        (target) => target.devicePath.toLowerCase() === partition.devicePath.toLowerCase(),
      );
      const next = selected
        ? veracryptDevices.filter(
            (target) => target.devicePath.toLowerCase() !== partition.devicePath.toLowerCase(),
          )
        : [
            ...veracryptDevices,
            {
              devicePath: partition.devicePath,
              diskNumber: partition.diskNumber,
              partitionNumber: partition.partitionNumber,
              partitionGuid: partition.partitionGuid,
              offsetBytes: partition.offsetBytes,
              sizeBytes: partition.sizeBytes,
              diskUniqueId: partition.diskUniqueId,
              label: partition.label,
            },
          ];
      onPatch({ cryptoEraseVeracryptDevices: next });
    },
    [onPatch, veracryptDevices],
  );

  return (
    <div className="sd-shred-folders">
      <div className="sd-shred-folders-header">
        <Icon icon="key" size={14} className="sd-shred-folders-icon" />
        <div className="sd-shred-folders-text">
          <div className="sd-shred-folders-label">Encrypted Volumes to Crypto-Erase on Lockdown</div>
        </div>
        <HowItWorks />
        <Button
          className="sd-bulk-btn"
          minimal
          small
          icon="refresh"
          text="Reload"
          loading={loading}
          onClick={refresh}
        />
      </div>

      <p className="sd-remove-users-warning">
        Selected volumes have their encryption keys destroyed when the cascade fires —
        via the sidebar button, hotkey, a distress phrase, the dead-man&rsquo;s switch, or the
        Calculator gate&rsquo;s destroy PIN. This only takes effect if the corresponding step
        (&ldquo;BitLocker Key Erase&rdquo; / &ldquo;VeraCrypt Header Destroy&rdquo; above) is enabled — nothing
        selected here means that step cleanly skips instead. There is no separate
        confirmation at trigger time: selecting a volume here, including the system
        drive, means it WILL be targeted next time.
      </p>

      {loading && volumes.length === 0 && (
        <div className="sd-remove-users-empty">
          <Spinner size={16} /> Looking for BitLocker volumes…
        </div>
      )}

      {!loading && volumes.length === 0 && (
        <EmptyState
          compact
          title="No BitLocker volumes detected — nothing to select here."
        />
      )}

      {volumes.length > 0 && (
        <div className="sd-remove-users-list">
          {volumes.map((v) => {
            const isOs = v.volumeType === "OperatingSystem";
            const checked = isBitlockerDriveSelected(bitlockerDrives, v.mountPoint);
            return (
              <label
                key={v.mountPoint}
                className="sd-row"
                onClick={(e) => {
                  const target = e.target as HTMLElement;
                  if (target.closest('button, [role="checkbox"]')) return;
                  e.preventDefault();
                  handleToggleDrive(v.mountPoint);
                }}
              >
                <Checkbox
                  className="sd-row-checkbox"
                  checked={checked}
                  onChange={() => handleToggleDrive(v.mountPoint)}
                />
                <div className="sd-row-content">
                  <div className="sd-row-label">
                    BitLocker {v.mountPoint}
                    {isOs && (
                      <Tooltip content={SYSTEM_TIP}>
                        <span className="ce-cascade-badge ce-cascade-badge--os">
                          SYSTEM
                          <Icon icon="info-sign" size={9} />
                        </span>
                      </Tooltip>
                    )}
                    {escrowRiskOf(v) && (
                      <Tooltip content={ESCROW_TIP}>
                        <span className="ce-cascade-badge ce-cascade-badge--escrow">
                          ESCROW RISK
                          <Icon icon="info-sign" size={9} />
                        </span>
                      </Tooltip>
                    )}
                  </div>
                </div>
              </label>
            );
          })}
        </div>
      )}

      <div className="sd-shred-folders-header" style={{ marginTop: "0.75rem" }}>
        <Icon icon="key" size={14} className="sd-shred-folders-icon" />
        <div className="sd-shred-folders-text">
          <div className="sd-shred-folders-label">VeraCrypt Containers</div>
        </div>
        <Button
          className="sd-bulk-btn"
          minimal
          small
          icon="plus"
          text="Add container…"
          onClick={addContainer}
        />
      </div>
      <p className="sd-remove-users-warning">
        Unmounted VeraCrypt containers leave no trace for the cascade to auto-discover
        (that&rsquo;s the point of their deniability), so they must be added here by path.
      </p>
      {veracryptPaths.length > 0 ? (
        <div className="sd-shred-folders-list">
          {veracryptPaths.map((p) => (
            <div key={p} className="sd-shred-folder-row">
              <Icon icon="key" size={12} className="sd-shred-folder-icon" />
              <span className="sd-shred-folder-path" title={p}>{p}</span>
              <Button
                minimal
                small
                icon="cross"
                onClick={() => removeContainer(p)}
                title="Remove from erase list"
              />
            </div>
          ))}
        </div>
      ) : (
        <EmptyState
          compact
          title="No containers listed — the VeraCrypt Header Destroy step will skip cleanly."
        />
      )}

      <div className="sd-shred-folders-header" style={{ marginTop: "0.75rem" }}>
        <Icon icon="database" size={14} className="sd-shred-folders-icon" />
        <div className="sd-shred-folders-text">
          <div className="sd-shred-folders-label">VeraCrypt Partitions</div>
        </div>
      </div>
      <p className="sd-remove-users-warning">
        Raw partitions are stored with their disk ID, GPT partition GUID, offset, and exact size.
        Lockdown rechecks that identity before overwriting any header bytes.
      </p>
      {partitions.length > 0 ? (
        <div className="sd-remove-users-list">
          {partitions.map((partition) => {
            const checked = veracryptDevices.some(
              (target) => target.devicePath.toLowerCase() === partition.devicePath.toLowerCase(),
            );
            const name = partition.driveLetter
              ? `${partition.driveLetter}:`
              : `Disk ${partition.diskNumber}, partition ${partition.partitionNumber}`;
            return (
              <label key={partition.devicePath} className="sd-row">
                <Checkbox
                  className="sd-row-checkbox"
                  checked={checked}
                  onChange={() => togglePartition(partition)}
                />
                <div className="sd-row-content">
                  <div className="sd-row-label">{name} · {partition.size}</div>
                  <div className="sd-row-description">
                    {partition.label || "Unlabelled"} · {partition.devicePath}
                  </div>
                </div>
              </label>
            );
          })}
        </div>
      ) : (
        <EmptyState compact title="No non-system writable partitions detected." />
      )}

      {(bitlockerDrives.length > 0 || veracryptPaths.length > 0 || veracryptDevices.length > 0) && (
        <Callout intent="danger" title="Armed">
          {bitlockerDrives.length > 0 && `${bitlockerDrives.length} BitLocker volume${bitlockerDrives.length === 1 ? "" : "s"}`}
          {bitlockerDrives.length > 0 && veracryptPaths.length > 0 && " and "}
          {veracryptPaths.length > 0 && `${veracryptPaths.length} VeraCrypt container${veracryptPaths.length === 1 ? "" : "s"}`}
          {(bitlockerDrives.length > 0 || veracryptPaths.length > 0) && veracryptDevices.length > 0 && " and "}
          {veracryptDevices.length > 0 && `${veracryptDevices.length} VeraCrypt partition${veracryptDevices.length === 1 ? "" : "s"}`}
          {" "}will have their keys destroyed the next time the cascade fires. There is no prompt at
          that moment.
        </Callout>
      )}
    </div>
  );
}
