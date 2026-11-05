// src/components/settings/ImportExportSettingsCard.tsx
//
// Backup & Restore: export the full settings.json to a user-chosen file, or
// import one to replace the current configuration. import_settings_cmd on
// the Rust side already preserves this machine's device_id/created_at, so
// restoring a backup taken on another machine is safe by design.

import { useState } from "react";
import { Button, Alert, Icon } from "@/components/ui/bp";
import { Popover, PopoverTrigger, PopoverContent } from "@/components/ui/popover";
import { Tooltip, TooltipTrigger, TooltipContent } from "@/components/ui/tooltip";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import SectionCard from "../shared/SectionCard";
import {
    type SettingsImportFile,
    writeSettingsExportFile,
    readSettingsImportFile,
    decryptSettingsImportFile,
} from "../../hooks/useSettings";
import { useAppState } from "../../context/AppContext";
import { useSettingsWriter } from "../../context/SettingsWriteContext";
import { showError, showSuccess } from "../../utils/toast";
import "./ImportExportSettingsCard.css";

// This exact sentence must stay verbatim (locked in an earlier compaction
// pass) — it now lives in the hover tooltip; the always-visible line is a
// short paraphrase so the card doesn't need a full-sentence callout.
const IMPORT_WARNING_FULL_TEXT =
    "Importing REPLACES your current configuration — every toggle and preference is overwritten. This device's identity (device ID, activation) is preserved automatically, so a backup from another machine is safe to restore.";

export default function ImportExportSettingsCard() {
    const { refreshSettings } = useAppState();
    const writer = useSettingsWriter();
    const [exporting, setExporting] = useState(false);
    const [importing, setImporting] = useState(false);
    const [pendingImportFile, setPendingImportFile] = useState<SettingsImportFile | null>(null);
    const [encryptedImport, setEncryptedImport] = useState<SettingsImportFile | null>(null);
    const [passwordMode, setPasswordMode] = useState<"export" | "import" | null>(null);
    const [password, setPassword] = useState("");
    const [confirmation, setConfirmation] = useState("");

    const runExport = async (backupPassword: string) => {
        setExporting(true);
        try {
            if (!await writeSettingsExportFile(backupPassword)) return;
            showSuccess("Settings exported.");
        } catch (err) {
            showError(err instanceof Error ? err.message : String(err));
        } finally {
            setExporting(false);
        }
    };

    const pickImportFile = async () => {
        setImporting(true);
        try {
            const file = await readSettingsImportFile();
            if (file?.encrypted) {
                setEncryptedImport(file);
                setPasswordMode("import");
            } else setPendingImportFile(file);
        } catch (err) {
            showError(err instanceof Error ? err.message : String(err));
        } finally {
            setImporting(false);
        }
    };

    const closePassword = () => {
        setPasswordMode(null);
        setPassword("");
        setConfirmation("");
        setEncryptedImport(null);
    };

    const submitPassword = async () => {
        if (passwordMode === "export") {
            const secret = password;
            closePassword();
            await runExport(secret);
        } else if (passwordMode === "import" && encryptedImport) {
            setImporting(true);
            try {
                const json = await decryptSettingsImportFile(encryptedImport.json, password);
                setPendingImportFile({ ...encryptedImport, json, encrypted: false });
                closePassword();
            } catch (err) {
                setPassword("");
                showError(err instanceof Error ? err.message : String(err));
            } finally {
                setImporting(false);
            }
        }
    };

    const confirmImport = async () => {
        const file = pendingImportFile;
        setPendingImportFile(null);
        if (!file) return;
        setImporting(true);
        try {
            await writer.import(file.json);
            await refreshSettings();
            showSuccess("Settings imported — this device's identity was kept as-is.");
        } catch (err) {
            showError(err instanceof Error ? err.message : String(err));
        } finally {
            setImporting(false);
        }
    };

    return (
        <SectionCard
            title="Backup & Restore"
            icon="import"
            className="import-export-settings-card"
            headerRight={(
                <div className="import-export-buttons">
                    <Button
                        icon="export"
                        text="Export"
                        small
                        loading={exporting}
                        onClick={() => setPasswordMode("export")}
                        aria-label="Export settings"
                    />
                    <Button
                        icon="import"
                        text="Import"
                        small
                        loading={importing}
                        onClick={pickImportFile}
                        aria-label="Import settings"
                    />

                    <Popover>
                        <PopoverTrigger className="import-export-info-trigger" aria-label="What Export and Import do">
                            <Icon icon="info-sign" size={13} />
                        </PopoverTrigger>
                        <PopoverContent align="start" className="import-export-info">
                            <div className="import-export-info__item">
                                <strong>Export</strong> — save your configuration in a password-protected backup.
                            </div>
                            <div className="import-export-info__item">
                                <strong>Import</strong> — load a previously exported JSON file and apply it to this device.
                            </div>
                        </PopoverContent>
                    </Popover>
                </div>
            )}
        >
            <Tooltip>
                <TooltipTrigger asChild>
                    <button type="button" className="import-export-warning">
                        <Icon icon="warning-sign" size={12} className="import-export-warning__icon" />
                        <span className="import-export-warning__text">Import replaces your current settings</span>
                    </button>
                </TooltipTrigger>
                <TooltipContent align="end" className="import-export-warning-tooltip">
                    {IMPORT_WARNING_FULL_TEXT}
                </TooltipContent>
            </Tooltip>

            <Dialog open={passwordMode !== null} onOpenChange={open => { if (!open && !exporting && !importing) closePassword(); }}>
                <DialogContent>
                    <DialogHeader>
                        <DialogTitle>{passwordMode === "export" ? "Protect your backup" : "Unlock your backup"}</DialogTitle>
                        <DialogDescription>
                            {passwordMode === "export"
                                ? "Choose a password of at least 12 characters. Keep it safe: the backup cannot be restored without it."
                                : "Enter the password used when this backup was created."}
                        </DialogDescription>
                    </DialogHeader>
                    <label htmlFor="settings-backup-password">Backup password</label>
                    <Input id="settings-backup-password" type="password" autoComplete={passwordMode === "export" ? "new-password" : "current-password"} maxLength={1024} autoFocus
                        value={password} onChange={event => setPassword(event.target.value)} />
                    {passwordMode === "export" && <>
                        <label htmlFor="settings-backup-confirmation">Confirm password</label>
                        <Input id="settings-backup-confirmation" type="password" autoComplete="new-password" maxLength={1024}
                            value={confirmation} onChange={event => setConfirmation(event.target.value)} />
                        {confirmation && password !== confirmation && <small role="status">Passwords do not match.</small>}
                    </>}
                    <DialogFooter>
                        <Button text="Cancel" onClick={closePassword} disabled={importing || exporting} />
                        <Button text={passwordMode === "export" ? "Continue" : "Unlock"} intent="primary"
                            onClick={submitPassword} loading={importing || exporting}
                            disabled={Array.from(password).length < 12 || (passwordMode === "export" && password !== confirmation)} />
                    </DialogFooter>
                </DialogContent>
            </Dialog>

            <Alert
                isOpen={!!pendingImportFile}
                cancelButtonText="Cancel"
                confirmButtonText="Replace settings"
                intent="danger"
                icon="import"
                onCancel={() => setPendingImportFile(null)}
                onConfirm={confirmImport}
                loading={importing}
            >
                <p>
                    Replace your current configuration with the contents of{" "}
                    <strong>{pendingImportFile?.fileName}</strong>? Every toggle and preference will be
                    overwritten. This cannot be undone.
                </p>
            </Alert>
        </SectionCard>
    );
}
