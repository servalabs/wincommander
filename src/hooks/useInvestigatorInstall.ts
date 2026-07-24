import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";

export interface InvestigatorInstallStatus {
  installed: boolean;
  install_dir: string;
  executable_path: string;
  version?: string | null;
}

interface InvestigatorReleaseManifest {
  version: string;
}

export default function useInvestigatorInstall(enabled: boolean) {
  const [status, setStatus] = useState<InvestigatorInstallStatus | null>(null);
  const [isBusy, setIsBusy] = useState(false);

  const refresh = useCallback(async () => {
    if (!enabled) {
      setStatus(null);
      return;
    }
    const next = await invoke<InvestigatorInstallStatus>("get_investigator_install_status");
    setStatus(next);
  }, [enabled]);

  useEffect(() => {
    void refresh().catch(() => setStatus(null));
  }, [refresh]);

  const launchOrInstall = useCallback(async () => {
    if (!enabled) throw new Error("An active Investigator subscription is required.");
    setIsBusy(true);
    try {
      const current = await invoke<InvestigatorInstallStatus>("get_investigator_install_status");
      let shouldInstall = !current.installed;
      try {
        const latest = await invoke<InvestigatorReleaseManifest>("fetch_investigator_manifest");
        shouldInstall ||= current.version !== latest.version;
      } catch (error) {
        // A verified installed pair remains launchable without network access.
        // A first install cannot proceed without its signed release manifest.
        if (!current.installed) throw error;
      }
      if (shouldInstall) await invoke("install_investigator_product");
      await invoke("launch_investigator_product");
      await refresh();
    } finally {
      setIsBusy(false);
    }
  }, [enabled, refresh]);

  return { status, isBusy, launchOrInstall };
}
