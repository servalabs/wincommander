import "./index.css";
import "./styles/v2-theme.css";
import "@fontsource/ibm-plex-sans/400.css";
import "@fontsource/ibm-plex-sans/600.css";
import "@fontsource/ibm-plex-mono/400.css";
import "@fontsource/ibm-plex-mono/600.css";
import "@fontsource/space-grotesk/600.css";
import "@fontsource/space-grotesk/700.css";
import { getCurrentWindow } from "@tauri-apps/api/window";
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

if (!hasNativeBackend) {
  void import("./entries/backendRequired").then(({ mountBackendRequired }) => mountBackendRequired());
} else if (windowLabel === "search-overlay") {
  void import("./entries/searchOverlay").then(({ mountSearchOverlay }) => mountSearchOverlay());
} else if (windowLabel === "notification-alerts") {
  void import("./entries/notificationAlerts").then(({ mountNotificationAlerts }) => mountNotificationAlerts());
} else {
  void import("./entries/mainWindow").then(({ mountMainWindow }) => mountMainWindow());
}
