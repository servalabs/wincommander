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
import { Button, Checkbox, Icon, Spinner } from "@/components/ui/bp";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import useBackend, { type BitLockerVolume } from "../../hooks/useBackend";
import { showError } from "../../utils/toast";
import {
  toggleBitlockerDrive,
  isBitlockerDriveSelected,
  addVeracryptPath,
  removeVeracryptPath,
} from "./cryptoEraseCascadeUtils";
import "./LockdownConfigSection.css";

interface Props {
  bitlockerDrives: string[];
  veracryptPaths: string[];
  onPatch: (patch: {
    cryptoEraseBitlockerDrives?: string[];
    cryptoEraseVeracryptPaths?: string[];
  }) => void;
}

export default function CryptoEraseTargetsSection({
  bitlockerDrives,
  veracryptPaths,
  onPatch,
}: Props) {
  const { getBitLockerVolumes } = useBackend();
  const [volumes, setVolumes] = useState<BitLockerVolume[]>([]);
  const [loading, setLoading] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    const res = await getBitLockerVolumes();
    setLoading(false);
    if (res.success && Array.isArray(res.data)) {
      setVolumes(res.data);
    } else if (!res.success) {
      showError(res.error || "Failed to load BitLocker volumes");
    }
  }, [getBitLockerVolumes]);

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

  return (
    <div className="sd-shred-folders">
      <div className="sd-shred-folders-header">
        <Icon icon="key" size={14} className="sd-shred-folders-icon" />
        <div className="sd-shred-folders-text">
          <div className="sd-shred-folders-label">Encrypted Volumes to Crypto-Erase on Lockdown</div>
        </div>
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

      {loading && volumes.length === 0 && <Spinner size={20} />}

      {!loading && volumes.length === 0 && (
        <div className="sd-remove-users-empty">No BitLocker volumes detected.</div>
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
                    {isOs && <span className="crypto-erase-badge crypto-erase-badge--os">SYSTEM</span>}
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
      {veracryptPaths.length > 0 && (
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
      )}
    </div>
  );
}
