import { useState } from "react";
import { useAppState } from "../../context/AppContext";
import { isPrivilegedWriteBlocked, MACHINE_SCOPE_ELEVATION_MESSAGE } from "../../lib/machineScopeElevation";
import { reportSettingsWriteFailure } from "../../lib/settingsWriteRecovery";
import { Switch } from "../ui/switch";

/** The last dashboard-tour stop asks before making Lockdown available. */
export default function LockdownTourChoice() {
  const { appSettings, systemInfo, patchAppSettings } = useAppState();
  const enabled = appSettings?.ideal?.privacy?.selfDestruct?.enabled === true;
  const enableBlocked = isPrivilegedWriteBlocked(true, systemInfo?.isAdmin);
  const [saving, setSaving] = useState(false);
  const [saveFailed, setSaveFailed] = useState(false);

  const setLockdownEnabled = async (nextEnabled: boolean) => {
    if (saving || (nextEnabled && enableBlocked)) return;
    setSaving(true);
    setSaveFailed(false);
    try {
      if (enabled !== nextEnabled) {
        await patchAppSettings({
          ideal: { privacy: { selfDestruct: { enabled: nextEnabled } } },
        } as any);
      }
    } catch (error) {
      setSaveFailed(true);
      reportSettingsWriteFailure(error);
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="lockdown-tour-choice">
      <div className="lockdown-tour-choice__control">
        <div>
          <div className="lockdown-tour-choice__label">Enable Lockdown</div>
          <p className="lockdown-tour-choice__description">
            Off by default. Turn it on to show the emergency control and arm its configured triggers.
          </p>
        </div>
        <Switch
          checked={enabled}
          onCheckedChange={(nextEnabled) => void setLockdownEnabled(nextEnabled)}
          disabled={saving || enableBlocked}
          aria-label="Enable Lockdown"
        />
      </div>
      {saving && <p className="lockdown-tour-choice__note" role="status">Saving Lockdown setting…</p>}
      {enableBlocked && (
        <p className="lockdown-tour-choice__note" role="status">
          {MACHINE_SCOPE_ELEVATION_MESSAGE}
        </p>
      )}
      {saveFailed && (
        <p className="lockdown-tour-choice__error" role="alert">
          WinCommander could not save this setting. Lockdown remains off; try again later.
        </p>
      )}
    </div>
  );
}
