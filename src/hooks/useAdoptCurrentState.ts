import { useEffect, useRef, useState } from "react";
import { useSettingsQuery } from "./queries/useSettingsQuery";
import { useSettingsWriter } from "../context/SettingsWriteContext";
import { getRadarDriftToggles } from "../registry";
import { useAuthMode } from "../context/AuthModeContext";
import { useAppState } from "../context/AppContext";
import { readPersonalSettingsStatus } from "../lib/startupHydration";
import { buildAdoptionPatch, createAdoptionAttempts } from "../lib/settingsAdoption";

export default function useAdoptCurrentState() {
  const { mode: authMode } = useAuthMode();
  const { data: appSettings } = useSettingsQuery();
  const writer = useSettingsWriter();
  const { personalSettingsStatus } = useAppState();
  const attempts = useRef(createAdoptionAttempts());
  const mounted = useRef(false);
  const retryTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [generation, setGeneration] = useState(0);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (retryTimer.current !== null) clearTimeout(retryTimer.current);
    };
  }, []);

  useEffect(() => {
    if (authMode === "decoy" || !appSettings?.app?.firstRunComplete) return;
    const status = readPersonalSettingsStatus(appSettings, personalSettingsStatus);
    if (status?.mode !== "service" || !status.canSave || status.recoveryRequired) return;
    const patch = buildAdoptionPatch(appSettings, getRadarDriftToggles());
    if (!patch) return;
    const key = JSON.stringify({ patch, current: appSettings.current });
    if (!attempts.current.begin(key)) return;
    // Only the native conditional receipt acknowledges an adoption attempt.
    void writer.adopt(patch).then(
      () => { attempts.current.finish(key, true); },
      () => { attempts.current.finish(key, false); },
    ).finally(() => {
      if (!mounted.current) return;
      retryTimer.current = setTimeout(() => {
        if (mounted.current) setGeneration(value => value + 1);
      }, 1000);
    });
  }, [authMode, appSettings, writer, generation, personalSettingsStatus]);
}
