import { Button, Icon, Tooltip, Switch, FormGroup, HTMLSelect, InputGroup, Dialog, Classes } from "@/components/ui/bp";
import { useCallback, useEffect, useState } from "react";
import useBackend from "../../hooks/useBackend";
import UniversalToggle from "../../components/shared/UniversalToggle";
import SectionCard from "../../components/shared/SectionCard";
import { useAppState } from "../../context/AppContext";
import { showSuccess, showError } from "../../utils/toast";
import type { RamDisk, RamDiskStatus, SystemRamInfo } from "../../hooks/useBackend";
import type { RamDiskAutostartSettings } from "../../types/settings";
import CreateRamDiskDialog from "./CreateRamDiskDialog";

function fmtMB(mb: number): string {
  if (mb >= 1024) {
    const gb = mb / 1024;
    return `${Number.isInteger(gb) ? gb : gb.toFixed(1)} GB`;
  }
  return `${mb} MB`;
}

function RamDisksSection() {
  const {
    testRamDiskInstalled,
    installRamDiskEngine,
    getRamDiskStatus,
    getSystemRamInfo,
    removeRamDisk,
    removeAllRamDisks,
    openRamDisk,
  } = useBackend();

  const [status, setStatus] = useState<RamDiskStatus | null>(null);
  const [sysRam, setSysRam] = useState<SystemRamInfo | null>(null);
  const [loading, setLoading] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [createOpen, setCreateOpen] = useState(false);
  const [dismountingAll, setDismountingAll] = useState(false);

  const { appSettings, patchAppSettings } = useAppState();
  const savedAutostart: RamDiskAutostartSettings = appSettings?.app?.vault?.ramdiskAutostart ?? {};
  const [autostartEnabled, setAutostartEnabled] = useState<boolean>(!!savedAutostart.enabled);
  const [asSizeMB, setAsSizeMB] = useState<number>(savedAutostart.sizeMB ?? 1024);
  const [asLetter, setAsLetter] = useState<string>(savedAutostart.driveLetter ?? "R");
  const [asFs, setAsFs] = useState<"NTFS" | "FAT32" | "exFAT">(savedAutostart.filesystem ?? "NTFS");
  const [asLabel, setAsLabel] = useState<string>(savedAutostart.label ?? "TEMP");
  const [asReadOnly, setAsReadOnly] = useState<boolean>(!!savedAutostart.readOnly);
  const [asSkipAfterLockdown, setAsSkipAfterLockdown] = useState<boolean>(!!savedAutostart.skipAfterLockdown);
  const [autostartSaving, setAutostartSaving] = useState(false);
  const [autostartConfigOpen, setAutostartConfigOpen] = useState(false);

  useEffect(() => {
    setAutostartEnabled(!!savedAutostart.enabled);
    setAsSizeMB(savedAutostart.sizeMB ?? 1024);
    setAsLetter(savedAutostart.driveLetter ?? "R");
    setAsFs(savedAutostart.filesystem ?? "NTFS");
    setAsLabel(savedAutostart.label ?? "TEMP");
    setAsReadOnly(!!savedAutostart.readOnly);
    setAsSkipAfterLockdown(!!savedAutostart.skipAfterLockdown);
  }, [
    savedAutostart.enabled,
    savedAutostart.sizeMB,
    savedAutostart.driveLetter,
    savedAutostart.filesystem,
    savedAutostart.label,
    savedAutostart.readOnly,
    savedAutostart.skipAfterLockdown,
  ]);

  const saveAutostart = useCallback(async (override?: Partial<RamDiskAutostartSettings>) => {
    const next: RamDiskAutostartSettings = {
      enabled: autostartEnabled,
      sizeMB: asSizeMB > 0 ? Math.round(asSizeMB) : 1024,
      driveLetter: asLetter,
      filesystem: asFs,
      label: asLabel || "TEMP",
      readOnly: asReadOnly,
      skipAfterLockdown: asSkipAfterLockdown,
      ...override,
    };
    setAutostartSaving(true);
    try {
      await patchAppSettings({ app: { vault: { ramdiskAutostart: next } } } as any);
      showSuccess(next.enabled ? "Autostart enabled — RAM disk will create on next launch." : "Autostart disabled.");
    } catch {
      showError("Failed to save autostart settings.");
    } finally {
      setAutostartSaving(false);
    }
  }, [asSizeMB, autostartEnabled, asLetter, asFs, asLabel, asReadOnly, asSkipAfterLockdown, patchAppSettings]);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const test = await testRamDiskInstalled();
      const installed = (() => {
        if (!test?.success) return false;
        const d = test.data as { installed?: boolean } | boolean | undefined;
        if (typeof d === "boolean") return d;
        return Boolean(d?.installed);
      })();

      if (!installed) {
        setStatus({ installed: false, disks: [] });
        return;
      }

      const [statusRes, ramRes] = await Promise.all([getRamDiskStatus(), getSystemRamInfo()]);
      if (statusRes?.success && statusRes.data) setStatus(statusRes.data);
      if (ramRes?.success && ramRes.data) setSysRam(ramRes.data);
    } finally {
      setLoading(false);
    }
  }, [testRamDiskInstalled, getRamDiskStatus, getSystemRamInfo]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const handleInstall = async () => {
    setInstalling(true);
    try {
      const r = await installRamDiskEngine();
      const d = r?.data as any;
      const ok = r?.success && d?.status !== 'error';
      if (ok) {
        showSuccess("RAM Disk Engine installed.");
        setTimeout(() => void refresh(), 1500);
      } else {
        showError(d?.error || r?.error || "Failed to install RAM Disk Engine.");
      }
    } finally {
      setInstalling(false);
    }
  };

  const handleDismount = async (letter: string) => {
    const r = await removeRamDisk(letter);
    if (r?.success) {
      showSuccess(`RAM disk ${letter} dismounted.`);
      void refresh();
    } else {
      // Operational RAM-disk result → Notifications tab, not System Alerts.
      showError(r?.error || `Failed to dismount ${letter}.`, undefined, { kind: "notification" });
    }
  };

  const handleDismountAll = async () => {
    if (!status?.disks?.length) return;
    setDismountingAll(true);
    try {
      const r = await removeAllRamDisks();
      if (r?.success) {
        showSuccess("All RAM disks dismounted.");
        void refresh();
      } else {
        showError(r?.error || "Failed to dismount RAM disks.", undefined, { kind: "notification" });
      }
    } finally {
      setDismountingAll(false);
    }
  };

  const isInstalled = status?.installed ?? true;

  if (!isInstalled) {
    return (
      <SectionCard title="RAM Disks" icon="database">
        <div className="vault-card install-prompt">
          <Icon icon="database" size={48} className="install-icon" />
          <h2>RAM Disk Engine Not Installed</h2>
          <p>RAM disk support is required for this feature. Install the engine to enable it.</p>
          <UniversalToggle
            label="Install Engine"
            description="Download and install the RAM disk engine."
            checked={installing}
            onChange={handleInstall}
            isAction
            severity="primary"
            loading={installing}
          />
        </div>
      </SectionCard>
    );
  }

  const disks: RamDisk[] = status?.disks || [];
  const totalUsedMB = disks.reduce((sum, d) => sum + Math.round(d.sizeBytes / (1024 * 1024)), 0);

  // Slider bounds for autostart size
  const sliderMin = 256;
  const sliderMax = sysRam && sysRam.totalMB > 0
    ? Math.max(1024, Math.round(sysRam.totalMB - 3072))
    : 32768;
  const fillPct = sliderMax > sliderMin
    ? Math.max(0, Math.min(100, ((asSizeMB - sliderMin) / (sliderMax - sliderMin)) * 100))
    : 0;

  return (
    <>
      {/* Single RAM Disks card — header + table + action row + collapsible
          autostart, on the RAM disks tab. */}
      <SectionCard
        title="RAM Disks"
        icon="database"
        headerRight={
          <div className="flex items-center gap-1">
            {sysRam && (
              <span className="vault-status-badge" style={{ background: 'var(--color-bg-tertiary)', color: 'var(--color-text-muted)', borderRadius: '2px', fontSize: '9px' }}>
                {(sysRam.freeMB / 1024).toFixed(1)} GB free
              </span>
            )}
            <Tooltip content="Dismount all RAM disks (data will be lost)" position="top">
              <Button
                icon="eject"
                intent="danger"
                minimal
                small
                loading={dismountingAll}
                disabled={disks.length === 0 || dismountingAll}
                onClick={handleDismountAll}
                aria-label="Dismount all RAM disks"
              />
            </Tooltip>
            <Tooltip content="Refresh RAM-disk status" position="top">
              <Button
                icon="refresh"
                minimal
                small
                loading={loading}
                disabled={loading}
                onClick={() => void refresh()}
                aria-label="Refresh RAM-disk status"
              />
            </Tooltip>
          </div>
        }
      >
        <p className="vault-card-status-line">
          {disks.length > 0
            ? `${disks.length} mounted · ${totalUsedMB >= 1024 ? `${(totalUsedMB / 1024).toFixed(1)} GB` : `${totalUsedMB} MB`} used`
            : "Memory-only — vanishes on shutdown"}
        </p>

        <div className="vault-card-action-row ramdisk-create-row">
          <Button
            icon="add"
            text="Create RAM Disk"
            intent="primary"
            onClick={() => setCreateOpen(true)}
            className="vault-action-btn vault-action-btn--primary"
          />
          <div className={`ramdisk-autostart-card ${autostartEnabled ? "is-enabled" : ""}`}>
            <div className="ramdisk-autostart-row">
              <div className="ramdisk-autostart-title">
                <Icon icon="automatic-updates" size={12} />
                <span>Startup RAM disk</span>
                {autostartEnabled && (
                  <Tooltip content="Edit auto-create settings">
                    <Button
                      minimal
                      small
                      icon="edit"
                      onClick={() => setAutostartConfigOpen(true)}
                      aria-label="Edit auto-create settings"
                      className="ramdisk-autostart-expand"
                    />
                  </Tooltip>
                )}
              </div>
              <Switch
                checked={autostartEnabled}
                disabled={autostartSaving}
                style={{ marginBottom: 0 }}
                onChange={(e) => {
                  const next = e.currentTarget.checked;
                  setAutostartEnabled(next);
                  // Open the modal on first enable so the user can confirm
                  // size / drive letter / filesystem before the spec lands.
                  setAutostartConfigOpen(next);
                  void saveAutostart({ enabled: next });
                }}
              />
            </div>
          </div>
        </div>

        {/* Active disks table OR empty state */}
        <div className={`vault-content ${disks.length === 0 ? "vault-content--empty" : ""}`}>
          {disks.length > 0 ? (
            <table className="volumes-table wc-table wc-table--striped">
              <thead>
                <tr>
                  <th>DRIVE</th>
                  <th>TYPE</th>
                  <th>SIZE</th>
                  <th>PROPERTIES</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                {disks.map((d) => (
                  <tr key={d.letter}>
                    <td className="mono-cell">
                      <span className="vault-active-dot" aria-hidden />
                      <strong>{d.letter}</strong>
                    </td>
                    <td><span className="type-badge">{d.type}</span></td>
                    <td className="path-cell">{d.size}</td>
                    <td className="path-cell">{d.properties || "—"}</td>
                    <td className="actions-cell">
                      <div style={{ display: "flex", gap: 4 }}>
                        <Tooltip content="Open in Explorer">
                          <Button minimal small icon="folder-open" onClick={() => openRamDisk(d.letter)} />
                        </Tooltip>
                        <Tooltip content="Dismount (data will be lost)">
                          <Button minimal small icon="eject" intent="danger" onClick={() => void handleDismount(d.letter)} />
                        </Tooltip>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          ) : (
            <div className="empty-state">
              <Icon icon="database" className="empty-icon" size={32} />
              <p>No RAM disks mounted</p>
            </div>
          )}
        </div>

      </SectionCard>

      {/* Autostart config — modal popup. Keeps the card height stable
          (the inline form previously made the card grow and left a blank
          gap beside the Encrypted Volumes card). The form is a clean
          vertical stack: Size (full-width) → Drive Letter + Filesystem
          (two columns) → Label (full-width) → Read-only + Save footer.
          Three across was cramped at 560px and made the labels overflow. */}
      <Dialog
        isOpen={autostartConfigOpen && autostartEnabled}
        onClose={() => setAutostartConfigOpen(false)}
        title="Auto-create RAM disk on startup"
        icon="automatic-updates"
        className={`ramdisk-autostart-dialog ${Classes.DARK}`}
        style={{ width: "min(95vw, 560px)" }}
      >
        <div className={Classes.DIALOG_BODY}>
          <div className="ramdisk-autostart-form ramdisk-autostart-form--modal">
            <div className="vault-range-wrapper">
              <div className="vault-range-label">
                Size
                <span className="vault-range-value">{fmtMB(asSizeMB)}</span>
              </div>
              <input
                type="range"
                className="vault-range"
                min={sliderMin}
                max={sliderMax}
                step={256}
                value={asSizeMB}
                onChange={(e) => setAsSizeMB(Number(e.target.value))}
                style={{ '--fill': `${fillPct.toFixed(1)}%` } as React.CSSProperties}
              />
              <div className="vault-range-limits">
                <span>256 MB</span>
                <span>{fmtMB(sliderMax)} max</span>
              </div>
            </div>

            {/* Drive Letter + Filesystem on one row; Label gets its own
                row below so the input has room to breathe and the user can
                see the full text they're typing. */}
            <div className="ramdisk-autostart-form-row">
              <FormGroup label="Drive Letter" labelFor="as-letter" className="ramdisk-autostart-field" style={{ marginBottom: 0 }}>
                <HTMLSelect
                  id="as-letter"
                  value={asLetter}
                  onChange={(e) => setAsLetter(e.target.value)}
                  fill
                  options={"DEFGHIJKLMNOPQRSTUVWXYZ".split("").map((l) => ({ value: l, label: `${l}:\\` }))}
                />
              </FormGroup>
              <FormGroup label="Filesystem" labelFor="as-fs" className="ramdisk-autostart-field" style={{ marginBottom: 0 }}>
                <HTMLSelect
                  id="as-fs"
                  value={asFs}
                  onChange={(e) => setAsFs(e.target.value as "NTFS" | "FAT32" | "exFAT")}
                  fill
                  options={["NTFS", "FAT32", "exFAT"].map((f) => ({ value: f, label: f }))}
                />
              </FormGroup>
            </div>

            <FormGroup label="Label" labelFor="as-label" className="ramdisk-autostart-field ramdisk-autostart-field--label" style={{ marginBottom: 0 }}>
              <InputGroup
                id="as-label"
                value={asLabel}
                maxLength={32}
                onChange={(e) => setAsLabel(e.target.value)}
              />
            </FormGroup>

            <div className="ramdisk-autostart-footer">
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                <Switch
                  checked={asReadOnly}
                  onChange={(e) => setAsReadOnly(e.currentTarget.checked)}
                  label="Read-only"
                  style={{ marginBottom: 0 }}
                />
              </div>
              <Button
                intent="primary"
                icon="floppy-disk"
                text={autostartSaving ? 'Saving…' : 'Save spec'}
                loading={autostartSaving}
                onClick={async () => {
                  await saveAutostart();
                  setAutostartConfigOpen(false);
                }}
              />
            </div>
          </div>
        </div>
      </Dialog>

      <CreateRamDiskDialog
        isOpen={createOpen}
        onClose={() => setCreateOpen(false)}
        onCreated={() => {
          setCreateOpen(false);
          void refresh();
        }}
        freeRamMB={sysRam?.freeMB ?? 0}
        totalRamMB={sysRam?.totalMB ?? 0}
      />
    </>
  );
}

export default RamDisksSection;
