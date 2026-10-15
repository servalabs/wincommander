import { useCallback, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppState } from "@/context/AppContext";
import type { AppSettings } from "@/types/settings";
import { persistQuickMountExplorerPreference } from "@/lib/quickMountExplorerPreference";

export default function useQuickMountExplorerPreference() {
  const { appSettings, patchAppSettings, refreshSettings } = useAppState();
  const [saving, setSaving] = useState(false);
  const inFlight = useRef(false);
  const save = useCallback(async (enabled: boolean) => {
    if (inFlight.current) return;
    inFlight.current = true;
    setSaving(true);
    try {
      await persistQuickMountExplorerPreference(enabled,
        value => patchAppSettings({ app: { vault: { quickMountOpenExplorerAfterMount: value } } }),
        async () => (await invoke<AppSettings>("get_settings", { refreshPersonal: true })).app.vault.quickMountOpenExplorerAfterMount);
      await refreshSettings();
    } finally { inFlight.current = false; setSaving(false); }
  }, [patchAppSettings, refreshSettings]);
  return { enabled: appSettings?.app.vault.quickMountOpenExplorerAfterMount === true, saving, save };
}
