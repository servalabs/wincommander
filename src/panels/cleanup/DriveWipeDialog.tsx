import { useState, useEffect } from "react";
import { Button, Checkbox, Dialog, DialogBody, DialogFooter, Spinner } from "@/components/ui/bp";
import { invoke } from "@tauri-apps/api/core";
import type { WipeDriveEntry } from "../../hooks/useBackend";
import useBackend from "../../hooks/useBackend";
import { showSuccess, showError } from "../../utils/toast";

interface Props {
  open: boolean;
  onClose: () => void;
}

function mediaLabel(entry: WipeDriveEntry): string {
  if (entry.isRemovable) return "Flash";
  if (entry.mediaType === "NVMe") return "NVMe";
  if (entry.mediaType === "SSD") return "SSD";
  if (entry.mediaType === "HDD") return "HDD";
  return "Unknown";
}

function wipeMethodLabel(entry: WipeDriveEntry): string {
  if (entry.isRemovable || entry.mediaType === "SSD" || entry.mediaType === "NVMe") {
    return "cipher /w + TRIM";
  }
  return "cipher /w (DoD 3-pass)";
}

// What we pass to the Pro handler as MediaType
function effectiveMediaType(entry: WipeDriveEntry): string {
  if (entry.isRemovable) return "Flash";
  return entry.mediaType || "Unknown";
}

export default function DriveWipeDialog({ open, onClose }: Props) {
  const { invokeUnallocatedSpaceErase } = useBackend();
  const [drives, setDrives] = useState<WipeDriveEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [wiping, setWiping] = useState(false);

  useEffect(() => {
    if (!open) return;
    setLoading(true);
    setSelected(new Set());
    invoke<WipeDriveEntry[]>("get_wipe_drive_list")
      .then(list => setDrives(Array.isArray(list) ? list : []))
      .catch(() => setDrives([]))
      .finally(() => setLoading(false));
  }, [open]);

  const toggleDrive = (letter: string) => {
    setSelected(prev => {
      const next = new Set(prev);
      if (next.has(letter)) next.delete(letter); else next.add(letter);
      return next;
    });
  };

  const handleWipe = async () => {
    if (!selected.size) return;
    const driveList = Array.from(selected).map(l => `${l}:`).join(", ");
    if (!window.confirm(
      `Overwrite free space on ${driveList}?\n\nThis may take 30+ minutes per drive and runs in the background.`
    )) return;

    setWiping(true);
    const failures: string[] = [];
    for (const letter of Array.from(selected)) {
      const drive = drives.find(d => d.letter === letter);
      const mt = drive ? effectiveMediaType(drive) : "Unknown";
      const res = await invokeUnallocatedSpaceErase(letter, mt);
      if (!res.success) failures.push(`${letter}: ${res.error ?? "unknown error"}`);
    }
    setWiping(false);
    onClose();
    if (failures.length) {
      showError(`Wipe failed for: ${failures.join("; ")}`);
    } else {
      showSuccess(`Free space wipe started on ${driveList} — running in background.`);
    }
  };

  const selectedCount = selected.size;

  return (
    <Dialog
      isOpen={open}
      onClose={onClose}
      title="Wipe Free Space"
      icon="delete"
      className="mount-dialog"
      isCloseButtonShown
      canEscapeKeyClose={!wiping}
      canOutsideClickClose={!wiping}
    >
      <DialogBody>
        <p className="text-[var(--text-dim)] text-sm mb-3">
          Select which drives to wipe. Unallocated blocks are overwritten so deleted
          files cannot be recovered. The appropriate method is chosen per drive type.
        </p>

        <div className="mb-4 rounded-[var(--r-lg)] border border-[var(--warn)]/30 bg-[var(--warn)]/8 px-3 py-2.5 text-xs text-[var(--text-dim)] leading-relaxed">
          <span className="font-semibold text-[var(--warn)]">Best-effort only.</span>{" "}
          On SSDs and flash drives, wear-levelling and over-provisioning mean the
          controller may keep copies of overwritten data in remapped blocks.
          For guaranteed erasure use{" "}
          <span className="font-mono text-[var(--text)]">ATA Secure Erase</span> /
          {" "}<span className="font-mono text-[var(--text)]">NVMe Format</span> via your
          drive manufacturer's tool or <span className="font-mono text-[var(--text)]">hdparm</span>.
          On HDDs, cipher&nbsp;/w performs a 3-pass overwrite (zero, complement, random) —
          adequate per NIST SP 800-88 for magnetic media, but classified data still
          requires physical destruction.
        </div>

        {loading ? (
          <div className="flex items-center justify-center gap-2 py-8 text-[var(--text-mute)] text-sm">
            <Spinner size={16} />
            Detecting drives…
          </div>
        ) : drives.length === 0 ? (
          <div className="py-8 text-center text-[var(--text-mute)] text-sm">
            No local drives found.
          </div>
        ) : (
          <div className="space-y-2">
            {drives.map(drive => {
              const checked = selected.has(drive.letter);
              const mlabel  = mediaLabel(drive);
              const wlabel  = wipeMethodLabel(drive);
              return (
                <label
                  key={drive.letter}
                  className={[
                    "flex items-center gap-3 rounded-[var(--r-xl)] border p-3 cursor-pointer",
                    "transition-colors duration-[var(--dur-fast)]",
                    checked
                      ? "border-[var(--accent-line)] bg-[var(--accent)]/10"
                      : "border-[var(--border)] hover:border-[var(--border-strong)]",
                  ].join(" ")}
                >
                  <Checkbox
                    checked={checked}
                    onChange={() => toggleDrive(drive.letter)}
                    disabled={wiping}
                  />
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 flex-wrap">
                      <span className="font-mono text-[15px] font-semibold text-[var(--text)]">
                        {drive.letter}:
                      </span>
                      {drive.label && (
                        <span className="text-sm text-[var(--text-dim)] truncate">{drive.label}</span>
                      )}
                      <span
                        className="text-[10px] font-mono font-medium px-1.5 py-0.5 rounded-[var(--r-sm)] border border-[var(--border-strong)] text-[var(--text-mute)]"
                      >
                        {mlabel}
                      </span>
                      {drive.isSystem && (
                        <span className="text-[10px] font-medium px-1.5 py-0.5 rounded-[var(--r-sm)] bg-[var(--warn)]/15 text-[var(--warn)] border border-[var(--warn)]/30">
                          System
                        </span>
                      )}
                      {drive.isRemovable && (
                        <span className="text-[10px] font-medium px-1.5 py-0.5 rounded-[var(--r-sm)] bg-[var(--ok)]/15 text-[var(--ok)] border border-[var(--ok)]/30">
                          Removable
                        </span>
                      )}
                    </div>
                    <div className="flex items-center gap-3 mt-1">
                      <span className="text-xs text-[var(--text-mute)]">
                        {drive.freeGB} GB free / {drive.totalGB} GB total
                      </span>
                      <span className="text-xs text-[var(--text-mute)] opacity-70">{wlabel}</span>
                    </div>
                  </div>
                </label>
              );
            })}
          </div>
        )}
      </DialogBody>

      <DialogFooter>
        <Button minimal onClick={onClose} disabled={wiping}>Cancel</Button>
        <Button
          intent="danger"
          icon={wiping ? undefined : "delete"}
          disabled={!selectedCount || loading || wiping}
          onClick={handleWipe}
        >
          {wiping ? (
            <span className="flex items-center gap-2">
              <Spinner size={13} />
              Starting…
            </span>
          ) : (
            `Wipe Selected${selectedCount ? ` (${selectedCount})` : ""}`
          )}
        </Button>
      </DialogFooter>
    </Dialog>
  );
}
