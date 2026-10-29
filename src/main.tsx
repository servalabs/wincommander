import "./index.css";
import "./styles/v2-theme.css";
import "@fontsource/ibm-plex-sans/400.css";
import "@fontsource/ibm-plex-sans/600.css";
import "@fontsource/ibm-plex-mono/400.css";
import "@fontsource/ibm-plex-mono/600.css";
import "@fontsource/space-grotesk/600.css";
import "@fontsource/space-grotesk/700.css";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { message } from "@tauri-apps/plugin-dialog";
import { applyMotionClass } from "./lib/motionPolicy";

const hasNativeBackend = Boolean((window as typeof window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__);

applyMotionClass();

const windowLabel = (() => {
  try {
    return getCurrentWindow().label ?? "main";
  } catch {
    return "main";
  }
})();
const isNotificationWindow = windowLabel === "notification-alerts"
  || new URLSearchParams(window.location.search).get("wc-window") === "notification-alerts";
const isSearchOverlayWindow = windowLabel === "search-overlay"
  || new URLSearchParams(window.location.search).get("wc-window") === "search-overlay";

// The alert window is created by the Rust backend. In dev mode Tauri can
// legitimately use its postMessage IPC fallback before it exposes the legacy
// `__TAURI_INTERNALS__` marker, so route this trusted window by label first.
// Otherwise it mounts the disconnected-dev page and never acknowledges queued
// security alerts.
if (isNotificationWindow) {
  void import("./entries/notificationAlerts").then(({ mountNotificationAlerts }) => mountNotificationAlerts());
} else if (isSearchOverlayWindow) {
  // Route this trusted native window before the dev IPC marker check. WebView2
  // can publish that marker after the window label on a cold first launch.
  void import("./entries/searchOverlay").then(({ mountSearchOverlay }) => mountSearchOverlay());
} else if (!hasNativeBackend) {
  void import("./entries/backendRequired").then(({ mountBackendRequired }) => mountBackendRequired());
} else {
  void import("./entries/mainWindow")
    .then(({ mountMainWindow }) => mountMainWindow())
    .catch(async (error: unknown) => {
      console.error("Unable to load the main window", error);
      // A code-loading failure is not recoverable by replaying the same broken
      // page.  Report it natively, then close cleanly instead of leaving a
      // white/empty window or offering a retry loop.
      await message(
        "WinCommander could not complete startup. Please install the latest WinCommander update.",
        { title: "WinCommander could not start", kind: "error" },
      );
      await getCurrentWindow().close();
    });
}
