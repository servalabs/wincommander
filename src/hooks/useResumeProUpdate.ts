import { useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import useProInstall, { isProVersionCompatible } from "./useProInstall";
import { canResumeProUpdate, clearPendingProUpdate, readPendingProUpdate } from "../lib/pendingProUpdate";
import { showError, showInfo } from "../utils/toast";
import { shouldAutomaticallyReplacePro } from "../lib/proUpdateDecision";

/** NSIS exits the old desktop; finish its requested Pro update after relaunch. */
export function useResumeProUpdate(entitled: boolean, elevated: boolean): boolean {
  const [version, setVersion] = useState<string | null>(null);
  const [checked, setChecked] = useState(false);
  const started = useRef(false);
  const finished = useRef(false);
  const enabled = !!version && entitled && elevated;
  const pro = useProInstall({ status: enabled, manifest: enabled, defender: false });

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        if (await invoke<boolean>("is_dev_build")) return;
        const current = await getVersion();
        if (!cancelled && canResumeProUpdate(readPendingProUpdate(localStorage), current)) setVersion(current);
      } finally {
        if (!cancelled) setChecked(true);
      }
    })().catch(() => { /* Normal update controls remain available if startup probing fails. */ });
    return () => { cancelled = true; };
  }, []);

  useEffect(() => {
    if (!enabled || finished.current) return;
    const fail = (message: string) => {
      finished.current = true;
      showError(`WinCommander was updated, but Pro still needs attention. ${message}`, undefined, { kind: "notification" });
    };
    if (pro.installState.kind === "error" && started.current) {
      fail(pro.installState.message);
      return;
    }
    if (pro.manifestError && !pro.manifest) {
      fail(pro.manifestError);
      return;
    }
    if (!pro.status || !pro.manifest) return;
    if (!pro.status.installed) {
      fail("Install Pro from License / Pro to continue.");
      return;
    }
    if (pro.status.local_sha256?.toLowerCase() === pro.manifest.sha256.toLowerCase()
      && isProVersionCompatible(pro.status.local_version, version)
      && (!started.current || pro.installState.kind === "installed")) {
      finished.current = true;
      clearPendingProUpdate(localStorage, version!);
      showInfo("WinCommander and its Pro component are up to date.", undefined, { kind: "notification" });
      return;
    }
    if (started.current || pro.installState.kind === "installing") return;
    if (!shouldAutomaticallyReplacePro(pro.status, pro.manifest)) {
      fail("Check License / Pro before replacing this component; a newer compatible Pro version could not be confirmed.");
      return;
    }
    started.current = true;
    pro.reset();
    void pro.install(false);
  }, [enabled, version, pro]);

  useEffect(() => {
    if (!enabled || (pro.status && (pro.manifest || pro.manifestError))) return;
    const timer = setTimeout(() => {
      if (finished.current) return;
      finished.current = true;
      showError("The remaining Pro update could not be checked. Open License / Pro to retry.", undefined, { kind: "notification" });
    }, 45_000);
    return () => clearTimeout(timer);
  }, [enabled, pro.status, pro.manifest, pro.manifestError]);

  // This continuation is the one automatic update attempt for this launch,
  // including failure: do not immediately retry through the ordinary worker.
  return !checked || enabled;
}
