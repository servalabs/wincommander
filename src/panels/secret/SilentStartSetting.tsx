import { useState } from "react";
import { Switch } from "../../components/ui/switch";
import { useAppState } from "../../context/AppContext";
import { reportSettingsWriteFailure } from "../../lib/settingsWriteRecovery";

export default function SilentStartSetting({ autostartEnabled }: { autostartEnabled: boolean }) {
    const { appSettings, patchAppSettings, personalSettingsStatus } = useAppState();
    const [saving, setSaving] = useState(false);
    const [controlVersion, setControlVersion] = useState(0);
    const silent = appSettings?.app.startSilentlyAtSignIn ?? true;
    const policyLocked = (appSettings?.policy.lockedPaths ?? []).some((rawPath) => {
        const path = rawPath.trim().toLowerCase();
        const target = "app.startsilentlyatsignin";
        return path === target || (path.length > 0 && target.startsWith(`${path}.`));
    });
    const disabled = !autostartEnabled || !appSettings || saving || policyLocked
        || personalSettingsStatus?.canSave === false;

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
                    {policyLocked ? "Set by your administrator." : saving ? "Saving…"
                        : "On: stay in the background at sign-in. Off: open maximized with the taskbar visible. Your lock and hide settings still apply."}
                </div>
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
