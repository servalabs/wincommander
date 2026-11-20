import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { useQueryClient } from "@tanstack/react-query";
import { useAppState } from "../context/AppContext";
import { useAuthMode } from "../context/AuthModeContext";
import { settingsKeys } from "../lib/settingsQueryPolicy";
import { createAsyncListenerScope } from "../lib/asyncListeners";
import {
  announceDeviceStateRefresh,
  createDeviceStateRefresher,
  DEVICE_STATE_CHANGED_EVENT,
  SETTINGS_CHANGED_EVENT,
} from "../lib/deviceStateRefresh";

/**
 * Re-reads device state as soon as something outside the UI changes it, such
 * as a remote command applied by the Pro agent, instead of at the next poll.
 * Only reads back: nothing from the events is shown or written.
 */
export default function DeviceStateBridge() {
  const queryClient = useQueryClient();
  const { refreshSettings } = useAppState();
  const { mode } = useAuthMode();
  const refreshSettingsRef = useRef(refreshSettings);
  refreshSettingsRef.current = refreshSettings;

  useEffect(() => {
    // The decoy view must never read, or react to changes in, the real device.
    if (mode === "decoy") return;
    const refresher = createDeviceStateRefresher({
      readSettings: async () => {
        await Promise.all([
          queryClient.invalidateQueries({ queryKey: settingsKeys.all }),
          refreshSettingsRef.current(),
        ]);
      },
      announceDevice: () => announceDeviceStateRefresh(),
      onError: (error) => console.warn("Device state refresh failed:", error),
    });
    const scope = createAsyncListenerScope((error) => console.warn("Device state listener failed:", error));
    void scope.listen((handler) => listen(SETTINGS_CHANGED_EVENT, handler), refresher.settingsChanged);
    void scope.listen((handler) => listen(DEVICE_STATE_CHANGED_EVENT, handler), refresher.deviceStateChanged);
    return () => {
      refresher.dispose();
      scope.dispose();
    };
  }, [mode, queryClient]);

  return null;
}
