import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { CAMERA_RECHECK_EVENT, cameraPresenceStore, cameraAccessState, createCameraPresenceProbe, hasNoDetectedCamera, type CameraPresence } from "./cameraPresenceProbe";

const readPresence = createCameraPresenceProbe(() => invoke("dashboard_camera_presence"));

/** Refresh hardware presence separately from the Windows camera permission. */
export function useCameraPresence(): {
  presence: CameraPresence;
  blocked: boolean | null;
  mixed: boolean;
  refreshAccess: () => Promise<void>;
  invalidateAccess: () => void;
} {
  const [presence, setPresence] = useState<CameraPresence>(() => cameraPresenceStore.get() ?? "unknown");
  const [blocked, setBlocked] = useState<boolean | null>(null);
  const [mixed, setMixed] = useState(false);
  const mounted = useRef(false);
  const accessGeneration = useRef(0);
  const invalidatePendingAccess = useCallback(() => { accessGeneration.current++; }, []);
  const invalidateAccess = useCallback(() => {
    accessGeneration.current++;
    setBlocked(null);
    setMixed(false);
  }, []);
  const refreshAccess = useCallback(async () => {
    const generation = ++accessGeneration.current;
    let timer: ReturnType<typeof setTimeout>;
    const deadline = new Promise<null>(resolve => { timer = setTimeout(() => resolve(null), 20_000); });
    const result = await Promise.race([
      invoke<{ error?: boolean; disabled?: boolean; app_access_mixed?: boolean; desktop_apps_allowed?: boolean | null }>(
        "run_backend_script", { command: "Get-AppCapabilityAccessStatus", params: { Capability: "webcam" } },
      ),
      deadline,
    ]).catch(() => null).finally(() => clearTimeout(timer));
    if (mounted.current && generation === accessGeneration.current) {
      setBlocked(result && !result.error ? cameraAccessState(result) : null);
      setMixed(!!result && !result.error && result.disabled === false && (result.app_access_mixed === true || result.desktop_apps_allowed === false));
    }
  }, []);

  useEffect(() => {
    let active = true;
    let inFlight = false;
    let recheckQueued = false;
    let accessInFlight = false;
    mounted.current = true;
    const unsubscribe = cameraPresenceStore.subscribe(value => { if (active) setPresence(value ?? "unknown"); });
    const refresh = async (force = false) => {
      if (inFlight) { if (force) recheckQueued = true; return; }
      if (!force && hasNoDetectedCamera(cameraPresenceStore.get())) return;
      inFlight = true;
      const generation = cameraPresenceStore.epoch();
      try {
        const result = await readPresence(generation);
        if (active) cameraPresenceStore.publish(result, generation);
      } catch {
        if (active) setPresence("unknown");
      } finally {
        inFlight = false;
        if (active && recheckQueued) { recheckQueued = false; void refresh(true); }
      }
    };
    void refresh();
    const checkAccess = () => {
      if (accessInFlight) return;
      accessInFlight = true;
      void refreshAccess().finally(() => { accessInFlight = false; });
    };
    checkAccess();
    const interval = window.setInterval(() => { void refresh(); checkAccess(); }, 30_000);
    const onFocus = () => { void refresh(); checkAccess(); };
    const onRecheck = () => { void refresh(true); checkAccess(); };
    window.addEventListener(CAMERA_RECHECK_EVENT, onRecheck);
    window.addEventListener("focus", onFocus);
    return () => {
      active = false;
      unsubscribe();
      mounted.current = false;
      invalidatePendingAccess();
      window.clearInterval(interval);
      window.removeEventListener("focus", onFocus);
      window.removeEventListener(CAMERA_RECHECK_EVENT, onRecheck);
    };
  }, [refreshAccess, invalidatePendingAccess]);

  return { presence, blocked, mixed, refreshAccess, invalidateAccess };
}
