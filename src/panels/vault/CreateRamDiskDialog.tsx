import { Button, Dialog, FormGroup, HTMLSelect, InputGroup, Switch } from "@/components/ui/bp";
import { useCallback, useEffect, useState } from "react";
import useBackend from "../../hooks/useBackend";
import { useTheme } from "../../context/ThemeContext";
import { showError, showSuccess } from "../../utils/toast";
import { MIN_RAM_DISK_SIZE_MB, normalizeRamDiskSizeMB } from "../../lib/ramDisk";
import DriveLetterPicker from "./DriveLetterPicker";
import useVaultDriveLetters from "@/hooks/useVaultDriveLetters";

interface Props {
  isOpen: boolean;
  onClose: () => void;
  onCreated: () => void;
  freeRamMB: number;
  totalRamMB: number;
}

const HEADROOM_MB = 3072;
const FILESYSTEMS = ["NTFS", "FAT32", "exFAT"] as const;
type Filesystem = (typeof FILESYSTEMS)[number];

function fmtMB(mb: number): string {
  if (mb >= 1024) {
    const gb = mb / 1024;
    return `${Number.isInteger(gb) ? gb : gb.toFixed(gb % 0.25 === 0 ? 2 : 1)} GB`;
  }
  return `${mb} MB`;
}

function CreateRamDiskDialog({ isOpen, onClose, onCreated, freeRamMB, totalRamMB }: Props) {
  const { theme } = useTheme();
  const { createRamDisk, getAvailableDriveLetters } = useBackend();

  const [sizeMB, setSizeMB] = useState(MIN_RAM_DISK_SIZE_MB);
  const [letter, setLetter] = useState("R");
  const driveList = useVaultDriveLetters();
  const [filesystem, setFilesystem] = useState<Filesystem>("NTFS");
  const [label, setLabel] = useState("TEMP");
  const [readOnly, setReadOnly] = useState(false);
  const [quick, setQuick] = useState(true);
  const [creating, setCreating] = useState(false);

  const ramCapMB = totalRamMB > 0 ? Math.max(64, totalRamMB - HEADROOM_MB) : 0;
  const sliderMin = MIN_RAM_DISK_SIZE_MB;
  const sliderMax = ramCapMB > 0 ? Math.max(sliderMin, ramCapMB) : 32768;
  const overCap = ramCapMB > 0 && sizeMB > ramCapMB;
  const fillPct = sliderMax > sliderMin
    ? Math.max(0, Math.min(100, ((sizeMB - sliderMin) / (sliderMax - sliderMin)) * 100))
    : 0;

  const reset = useCallback(() => {
    setSizeMB(MIN_RAM_DISK_SIZE_MB);
    setLetter("R");
    setFilesystem("NTFS");
    setLabel("TEMP");
    setReadOnly(false);
    setQuick(true);
  }, []);

  useEffect(() => {
    if (!isOpen) return;
    reset();
  }, [isOpen, reset]);
  useEffect(() => {
    if (!driveList.loading && !driveList.unavailable) setLetter(current => driveList.letters.includes(current) ? current : driveList.letters[0] ?? "");
  }, [driveList.letters, driveList.loading, driveList.unavailable]);

  const handleCreate = async () => {
    if (sizeMB < MIN_RAM_DISK_SIZE_MB) {
      showError(`RAM disks must be at least ${MIN_RAM_DISK_SIZE_MB} MB.`);
      return;
    }
    setCreating(true);
    try {
      const available = await getAvailableDriveLetters();
      if (!available.success || !available.data?.letters.includes(letter)) {
        showError("The selected drive letter could not be confirmed free. Refresh the drive list before creating a RAM disk.");
        void driveList.refresh();
        return;
      }
      const r = await createRamDisk({
        SizeMB: normalizeRamDiskSizeMB(sizeMB),
        DriveLetter: letter,
        Filesystem: filesystem,
        Label: label || "TEMP",
        ReadOnly: readOnly,
        Quick: quick,
      });
      if (r?.success) {
        showSuccess(`RAM disk created at ${letter}:.`);
        onCreated();
      } else {
        // Operational RAM-disk result → Notifications tab, not System Alerts.
        showError(r?.error || "Failed to create RAM disk.", undefined, { kind: "notification" });
      }
    } catch (error) {
      showError(error instanceof Error ? error.message : "Failed to create RAM disk.", undefined, { kind: "notification" });
    } finally {
      setCreating(false);
    }
  };

  return (
    <Dialog
      isOpen={isOpen}
      onClose={onClose}
      title="Create RAM Disk"
      className={`mount-dialog ${theme === "light" ? "light" : ""}`}
      backdropProps={{
        style: {
          backgroundColor: theme === "light" ? "rgba(0, 0, 0, 0.4)" : "rgba(0, 0, 0, 0.7)",
          backdropFilter: "blur(4px)",
        },
      }}
    >
      <div className="wc-dialog-body">
        {/* Size slider */}
        <div className="vault-range-wrapper" style={{ marginBottom: 20 }}>
          <div className="vault-range-label">
            Size
            <span className="vault-range-value">{fmtMB(sizeMB)}</span>
          </div>
          <input
            aria-label="RAM disk size"
            type="range"
            className="vault-range"
            min={sliderMin}
            max={sliderMax}
            step={256}
            value={sizeMB}
            onChange={(e) => setSizeMB(Number(e.target.value))}
            style={{ '--fill': `${fillPct.toFixed(1)}%` } as React.CSSProperties}
          />
          <div className="vault-range-limits">
            <span>{MIN_RAM_DISK_SIZE_MB} MB</span>
            {totalRamMB > 0 ? (
              <span className={overCap ? "vault-range-over-cap" : ""}>
                {overCap
                  ? `Over cap — max ${fmtMB(ramCapMB)}`
                  : `Total ${(totalRamMB / 1024).toFixed(1)} GB · ${(freeRamMB / 1024).toFixed(1)} GB free`}
              </span>
            ) : (
              <span>{fmtMB(sliderMax)} max</span>
            )}
          </div>
        </div>

        <FormGroup label="Drive Letter" labelFor="ramdisk-letter">
          <DriveLetterPicker
            id="ramdisk-letter"
            value={letter}
            onChange={setLetter}
            letters={driveList.letters}
            loading={driveList.loading}
            unavailable={driveList.unavailable}
            onRefresh={() => void driveList.refresh()}
          />
        </FormGroup>

        <FormGroup label="Filesystem" labelFor="ramdisk-fs">
          <HTMLSelect
            id="ramdisk-fs"
            value={filesystem}
            onChange={(e) => setFilesystem(e.target.value as Filesystem)}
            fill
            options={FILESYSTEMS.map((f) => ({ value: f, label: f }))}
          />
        </FormGroup>

        <FormGroup label="Volume Label" labelFor="ramdisk-label">
          <InputGroup
            id="ramdisk-label"
            value={label}
            autoComplete="off"
            maxLength={32}
            onChange={(e) => setLabel(e.target.value)}
          />
        </FormGroup>

        <div style={{ display: "flex", gap: 24, marginTop: 8 }}>
          <Switch checked={quick} onChange={(e) => setQuick(e.currentTarget.checked)} label="Quick format" />
          <Switch checked={readOnly} onChange={(e) => setReadOnly(e.currentTarget.checked)} label="Read-only" />
        </div>
      </div>

      <div className="mount-dialog-footer">
        <Button icon="cross" text="CANCEL" onClick={onClose} minimal className="modal-cancel-btn" />
        <Button
          icon="add"
          text="CREATE"
          onClick={handleCreate}
          loading={creating}
          disabled={sizeMB < MIN_RAM_DISK_SIZE_MB || overCap || creating || driveList.loading || driveList.unavailable || !driveList.letters.includes(letter)}
          className="modal-primary-btn"
        />
      </div>
    </Dialog>
  );
}

export default CreateRamDiskDialog;
