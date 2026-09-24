import { useEffect, useState } from "react";
import { useAppState } from "../../context/AppContext";
import { isPrivilegedWriteBlocked, MACHINE_SCOPE_ELEVATION_MESSAGE } from "../../lib/machineScopeElevation";
import { reportSettingsWriteFailure } from "../../lib/settingsWriteRecovery";
import { setLockdownChoicePendingEnabled } from "../../lib/tourActive";
import { Switch } from "../ui/switch";

/** The last dashboard-tour stop asks before making Lockdown available. */
export default function LockdownTourChoice() {
  const { appSettings, systemInfo, patchAppSettings } = useAppState();
  const persistedEnabled = appSettings?.ideal?.privacy?.selfDestruct?.enabled === true;
  const enableBlocked = isPrivilegedWriteBlocked(true, systemInfo?.isAdmin);
  const [saving, setSaving] = useState(false);
  const [saveFailed, setSaveFailed] = useState(false);
  const [optimisticEnabled, setOptimisticEnabled] = useState<boolean | null>(null);
  const [savedPendingEnabled, setSavedPendingEnabled] = useState<boolean | null>(null);
  const enabled = optimisticEnabled ?? persistedEnabled;

  useEffect(() => {
    if (savedPendingEnabled === null || persistedEnabled !== savedPendingEnabled) return;
    setOptimisticEnabled(null);
    setLockdownChoicePendingEnabled(null);
    setSavedPendingEnabled(null);
  }, [persistedEnabled, savedPendingEnabled]);

  const setLockdownEnabled = async (nextEnabled: boolean) => {
    if (saving || (nextEnabled && enableBlocked)) return;
    if (enabled === nextEnabled) return;
    setSaving(true);
    setSaveFailed(false);
    // Respond to the switch immediately while the native settings write is in
    // flight. The rail uses this same tour-scoped value to reveal its control
    // immediately, but holds that control disabled until the write completes.
    setOptimisticEnabled(nextEnabled);
    setLockdownChoicePendingEnabled(nextEnabled);
    try {
      // AppContext queues this native settings write independently of the
      // tour component, so closing the tour does not cancel the backend save.
      await patchAppSettings({
        ideal: { privacy: { selfDestruct: { enabled: nextEnabled } } },
      } as any);
      // Keep the rail button disabled until AppContext has observed the saved
      // value. This avoids a brief gap where the ON preview can be clicked
      // before the persisted Lockdown control is safe to use.
      setSavedPendingEnabled(nextEnabled);
    } catch (error) {
      setOptimisticEnabled(null);
      setLockdownChoicePendingEnabled(null);
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
          WinCommander could not save this setting. Your saved choice is unchanged; try again later.
        </p>
      )}
    </div>
  );
}
