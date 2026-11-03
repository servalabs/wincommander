import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { cameraAccessState, createCameraPresenceProbe, type CameraPresence } from "./cameraPresenceProbe";

const readPresence = createCameraPresenceProbe(() => invoke("dashboard_camera_presence"));

/** Refresh hardware presence separately from the Windows camera permission. */
export function useCameraPresence(): {
  presence: CameraPresence;
  blocked: boolean | null;
  refreshAccess: () => Promise<void>;
  invalidateAccess: () => void;
} {
  const [presence, setPresence] = useState<CameraPresence>("unknown");
  const [blocked, setBlocked] = useState<boolean | null>(null);
  const mounted = useRef(false);
  const accessGeneration = useRef(0);
  const invalidatePendingAccess = useCallback(() => { accessGeneration.current++; }, []);
  const invalidateAccess = useCallback(() => {
    accessGeneration.current++;
    setBlocked(null);
  }, []);
  const refreshAccess = useCallback(async () => {
    const generation = ++accessGeneration.current;
    if (mounted.current) setBlocked(null);
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
    }
  }, []);

  useEffect(() => {
    let active = true;
    let inFlight = false;
    let accessInFlight = false;
    mounted.current = true;
    const refresh = async () => {
      if (inFlight) return;
      inFlight = true;
      try {
        const result = await readPresence();
        if (active) setPresence(result);
      } catch {
        if (active) setPresence("unknown");
      } finally {
        inFlight = false;
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
    window.addEventListener("focus", onFocus);
    return () => {
      active = false;
      mounted.current = false;
      invalidatePendingAccess();
      window.clearInterval(interval);
      window.removeEventListener("focus", onFocus);
    };
  }, [refreshAccess, invalidatePendingAccess]);

  return { presence, blocked, refreshAccess, invalidateAccess };
}
