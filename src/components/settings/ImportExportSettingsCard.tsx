// src/components/settings/ImportExportSettingsCard.tsx
//
// Backup & Restore: export the full settings.json to a user-chosen file, or
// import one to replace the current configuration. import_settings_cmd on
// the Rust side already preserves this machine's device_id/created_at, so
// restoring a backup taken on another machine is safe by design.

import { useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { Button, Alert } from "@/components/ui/bp";
import SectionCard from "../shared/SectionCard";
import UniversalCallout from "../shared/UniversalCallout";
import {
    exportSettings,
    importSettings,
    writeSettingsExportFile,
    readSettingsImportFile,
} from "../../hooks/useSettings";
import { useAppState } from "../../context/AppContext";
import { showError, showSuccess } from "../../utils/toast";
import "./ImportExportSettingsCard.css";

const SETTINGS_FILE_FILTER = [{ name: "WinCommander settings", extensions: ["json"] }];

function defaultExportFileName(): string {
    const date = new Date().toISOString().slice(0, 10);
    return `wincommander-settings-${date}.json`;
}

export default function ImportExportSettingsCard() {
    const { refreshSettings } = useAppState();
    const [exporting, setExporting] = useState(false);
    const [importing, setImporting] = useState(false);
    const [pendingImportPath, setPendingImportPath] = useState<string | null>(null);

    const runExport = async () => {
        setExporting(true);
        try {
            const json = await exportSettings();
            const path = await save({ defaultPath: defaultExportFileName(), filters: SETTINGS_FILE_FILTER });
            if (!path) return;
            await writeSettingsExportFile(path, json);
            showSuccess("Settings exported.");
        } catch (err) {
            showError(err instanceof Error ? err.message : String(err));
        } finally {
            setExporting(false);
        }
    };

    const pickImportFile = async () => {
        const picked = await open({ multiple: false, filters: SETTINGS_FILE_FILTER });
        if (typeof picked === "string") setPendingImportPath(picked);
    };

    const confirmImport = async () => {
        const path = pendingImportPath;
        setPendingImportPath(null);
        if (!path) return;
        setImporting(true);
        try {
            const json = await readSettingsImportFile(path);
            await importSettings(json);
            await refreshSettings();
            showSuccess("Settings imported — this device's identity was kept as-is.");
        } catch (err) {
            showError(err instanceof Error ? err.message : String(err));
        } finally {
            setImporting(false);
        }
    };

    return (
        <SectionCard title="Backup & Restore" icon="import" className="import-export-settings-card">
            <div className="import-export-stack">
                <div className="import-export-row">
                    <div className="import-export-row__text">
                        <div className="import-export-row__title">Export settings</div>
                        <div className="import-export-row__desc">
                            Save your full configuration to a JSON file you choose.
                        </div>
                    </div>
                    <Button
                        icon="export"
                        text="Export"
                        loading={exporting}
                        onClick={runExport}
                        aria-label="Export settings"
                    />
                </div>

                <div className="import-export-row">
                    <div className="import-export-row__text">
                        <div className="import-export-row__title">Import settings</div>
                        <div className="import-export-row__desc">
                            Load a previously exported JSON file and apply it to this device.
                        </div>
                    </div>
                    <Button
                        icon="import"
                        text="Import"
                        loading={importing}
                        onClick={pickImportFile}
                        aria-label="Import settings"
                    />
                </div>

                <UniversalCallout
                    message="Importing REPLACES your current configuration — every toggle and preference is overwritten. This device's identity (device ID, activation) is preserved automatically, so a backup from another machine is safe to restore."
                    intent="warning"
                />
            </div>

            <Alert
                isOpen={!!pendingImportPath}
                cancelButtonText="Cancel"
                confirmButtonText="Replace settings"
                intent="danger"
                icon="import"
                onCancel={() => setPendingImportPath(null)}
                onConfirm={confirmImport}
                loading={importing}
            >
                <p>
                    Replace your current configuration with the contents of{" "}
                    <strong>{pendingImportPath}</strong>? Every toggle and preference will be
                    overwritten. This cannot be undone.
                </p>
            </Alert>
        </SectionCard>
    );
}
