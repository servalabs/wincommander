import { useState } from "react";
import { Switch } from "../../components/ui/switch";
import { useAppState } from "../../context/AppContext";
import { reportSettingsWriteFailure } from "../../lib/settingsWriteRecovery";
import useProcessElevation from "../../hooks/useProcessElevation";

export default function SilentStartSetting({ autostartEnabled }: { autostartEnabled: boolean }) {
    const { appSettings, patchAppSettings, personalSettingsStatus } = useAppState();
    const { state: privileges, retry: retryPrivileges } = useProcessElevation();
    const [saving, setSaving] = useState(false);
    const [controlVersion, setControlVersion] = useState(0);
    const silent = appSettings?.app?.startSilentlyAtSignIn ?? true;
    const policyLocked = (appSettings?.policy?.lockedPaths ?? []).some((rawPath) => {
        const path = rawPath.trim().toLowerCase();
        const target = "app.startsilentlyatsignin";
        return path === target || (path.length > 0 && target.startsWith(`${path}.`));
    });
    const disabled = !autostartEnabled || !appSettings?.app || !appSettings?.policy || saving || policyLocked
        || personalSettingsStatus?.canSave === false || privileges !== "administrator";

    const save = async (next: boolean) => {
        if (disabled) return;
        setSaving(true);
        setControlVersion((version) => version + 1);
        try {
            await patchAppSettings({ app: { startSilentlyAtSignIn: next } });
        } catch (error) {
            reportSettingsWriteFailure(error);
        } finally {
            setSaving(false);
            setControlVersion((version) => version + 1);
        }
    };

    return (
        <div className="dgz-tile-row">
            <div className="dgz-tile-body">
                <div className="dgz-tile-title" id="silent-start-label">Start silently in tray</div>
                <div className="dgz-tile-desc" id="silent-start-description">
                    {!appSettings?.app || !appSettings?.policy ? "Waiting for this PC’s startup settings…"
                        : policyLocked ? "Set by your administrator." : saving ? "Saving…"
                        : "For everyone on this PC. On: stay in the tray at sign-in. Off: open maximized with the taskbar visible. Applies at the next sign-in; your lock and hide settings still apply."}
                </div>
                {!policyLocked && privileges === "standard" && <div className="dgz-tile-desc">Open WinCommander as administrator to change this PC-wide setting.</div>}
                {privileges === "checking" && <div className="dgz-tile-desc">Checking permission to change this PC-wide setting…</div>}
                {privileges === "unknown" && <button type="button" className="dgz-autostart-retry" onClick={retryPrivileges}>Check permission again</button>}
            </div>
            <Switch
                key={`silent-start-${silent}-${controlVersion}`}
                checked={silent}
                disabled={disabled}
                onCheckedChange={save}
                aria-labelledby="silent-start-label"
                aria-describedby="silent-start-description"
            />
        </div>
    );
}
