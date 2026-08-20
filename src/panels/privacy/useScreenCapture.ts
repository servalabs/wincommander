// src/panels/privacy/useScreenCapture.ts
//
// useScreenCapture — business logic for the Privacy → Screen Capture
// section (#5). Keeps the component declarative: it owns the recent-hit
// list, the live detector status, and the two toggle handlers, persisting
// each toggle into `appSettings.ideal.privacy.screenCapture`.
//
// Two halves, both Paid:
//   • DETECTION — start/stop the Pro sidecar poller that watches for
//     known screen-capture tools (OBS, ShareX, Bandicam, …). Best-effort
//     process-presence heuristic. Drives `detectionEnabled`.
//   • PROTECTION — apply SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)
//     to WinCommander's OWN window so it renders black in screenshots /
//     recordings / screen-share. Runs in the Free process. Drives
//     `protectWindow`.
//
// Errors are surfaced to the caller (not toasted here) so the component
// can render an inline upsell/degradation message.

import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface ScreenCaptureHit {
  tool: string;
  processName: string;
  /** "high" | "low" */
  confidence: string;
  detectedAt: string;
}

export interface ScreenCaptureStatus {
  running: boolean;
  lastTick: string | null;
}

export interface CaptureProtectionStatus {
  enabled: boolean;
  scope: "wincommander-main-window";
  mode: "exclude-from-capture" | "none";
  limitation: string;
}

const RECENT_CAP = 30;

export interface UseScreenCapture {
  recent: ScreenCaptureHit[];
  status: ScreenCaptureStatus | null;
  busy: boolean;
  error: string | null;
  toggleDetection: (next: boolean) => Promise<void>;
  toggleProtection: (next: boolean) => Promise<void>;
  clearRecent: () => Promise<void>;
}

/** Normalise paid-gate errors to a friendly upsell line; pass others through. */
function friendlyError(err: unknown): string {
  const msg = String(err);
  if (msg.includes("PAID:") || msg.toLowerCase().includes("paid")) {
    return "WinCommander Pro required for screen-capture detection.";
  }
  return msg;
}

export default function useScreenCapture(
  detectionEnabled: boolean,
  onPersist: (patch: { detectionEnabled?: boolean; protectWindow?: boolean }) => void,
): UseScreenCapture {
  const [recent, setRecent] = useState<ScreenCaptureHit[]>([]);
  const [status, setStatus] = useState<ScreenCaptureStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [s, r] = await Promise.all([
        invoke<ScreenCaptureStatus>("screen_capture_watch_status"),
        invoke<ScreenCaptureHit[]>("get_recent_screen_capture"),
      ]);
      setStatus(s);
      setRecent([...r].reverse());
    } catch {
      // Detector not armed / Pro absent — leave panel in its idle state
      // rather than surfacing a noisy error on a routine refresh.
      setStatus((prev) => prev ?? { running: false, lastTick: null });
    }
  }, []);

  // Live updates: a new hit pushes to the top of the recent list and
  // refreshes status (lastTick moves). Only mounted while the section is
  // on screen — the toast in BackgroundPollers is the always-on surface.
  useEffect(() => {
    void refresh();
    let unlisten: UnlistenFn | null = null;
    (async () => {
      unlisten = await listen<ScreenCaptureHit>("screen-capture-detected", (e) => {
        setRecent((prev) => [e.payload, ...prev].slice(0, RECENT_CAP));
        void refresh();
      });
    })();
    return () => {
      if (unlisten) unlisten();
    };
  }, [refresh]);

  const toggleDetection = useCallback(
    async (next: boolean) => {
      setBusy(true);
      setError(null);
      try {
        if (next) {
          const s = await invoke<ScreenCaptureStatus>("start_screen_capture_watch");
          setStatus(s);
        } else {
          await invoke("stop_screen_capture_watch");
          await refresh();
        }
        onPersist({ detectionEnabled: next });
      } catch (err) {
        setError(friendlyError(err));
      } finally {
        setBusy(false);
      }
    },
    [onPersist, refresh],
  );

  const toggleProtection = useCallback(
    async (next: boolean) => {
      setBusy(true);
      setError(null);
      try {
        await invoke<CaptureProtectionStatus>("set_capture_protection", { enabled: next });
        onPersist({ protectWindow: next });
      } catch (err) {
        // Pre-19041 builds / unlicensed: report, leave the window visible.
        setError(friendlyError(err));
      } finally {
        setBusy(false);
      }
    },
    [onPersist],
  );

  const clearRecent = useCallback(async () => {
    try {
      await invoke("clear_recent_screen_capture");
    } catch {
      /* best-effort */
    }
    setRecent([]);
  }, []);

  // Keep status live while the detector is meant to be running.
  useEffect(() => {
    if (!detectionEnabled) return;
    const id = setInterval(() => void refresh(), 30_000);
    return () => clearInterval(id);
  }, [detectionEnabled, refresh]);

  return { recent, status, busy, error, toggleDetection, toggleProtection, clearRecent };
}
