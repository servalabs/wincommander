import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import App from "./App";
import "./index.css";
// V2 theme override layer — MUST load after index.css so it wins the cascade.
import "./styles/v2-theme.css";
// Self-hosted V2 fonts (no runtime CDN fetch — Tauri offline / privacy / CSP).
import "@fontsource/ibm-plex-sans/400.css";
import "@fontsource/ibm-plex-sans/500.css";
import "@fontsource/ibm-plex-sans/600.css";
import "@fontsource/ibm-plex-mono/400.css";
import "@fontsource/ibm-plex-mono/500.css";
import "@fontsource/ibm-plex-mono/600.css";
import "@fontsource/space-grotesk/500.css";
import "@fontsource/space-grotesk/600.css";
import "@fontsource/space-grotesk/700.css";
import { ThemeProvider } from "./context/ThemeContext";
import EverythingSearchBar from "./components/EverythingSearchBar";
import CustomNotificationWindow from "./components/CustomNotificationWindow";
import ExternalNotificationBridge from "./components/ExternalNotificationBridge";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { initUniversalLogging } from "./lib/logger";
import { applyMotionClass } from "./lib/motionPolicy";
import GlobalErrorBoundary from "./components/ErrorBoundary";
import { AppConfirmProvider } from "./components/shared/AppConfirmDialog";
import { UsbHidApprovalProvider } from "./context/UsbHidApprovalContext";
import { logo as brandLogo } from "./assets";

const hasNativeBackend = Boolean((window as typeof window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__);

// A plain browser can render the React shell but cannot execute a single
// WinCommander command. Do not present empty/default cards as if they were real
// host results: that looks like a working but data-less application. The
// explicit UI-audit entry installs Tauri mocks before importing this module, so
// it remains available for fixture-driven visual tests.
if (hasNativeBackend) {
  initUniversalLogging();
  console.log('[App] Frontend environment initialized.');
}

// Resolve + apply the motion preference BEFORE first render so the splash
// screen and every component see the correct `wc-no-motion` state immediately.
// Honors an explicit user choice, else OS reduced-motion, else the low-spec
// hardware default (< 4 cores or < 8 GB RAM → animations off).
applyMotionClass();

// Suppress Tauri WebView's default F12 → DevTools shortcut so operators
// can use F12 as a flow trigger (KeySequenceTrigger pre-shipped default
// is F12 ×3). The system-wide WH_KEYBOARD_LL hook still receives the key
// — we're only blocking the WebView-level default-action handler.
//
// Capture-phase listener so we win the race with the WebView's own
// keydown handler. `preventDefault` does the actual suppression;
// `stopImmediatePropagation` keeps it from triggering any sibling
// handlers on the same target.
window.addEventListener(
  'keydown',
  (e) => {
    if (e.key === 'F12' && !e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey) {
      e.preventDefault();
      e.stopImmediatePropagation();
    }
  },
  true,
);

// ── React Query client ──────────────────────────────────────────────
// staleTime: 30s — probes are expensive (PowerShell → OS), don't re-run
// unless the user explicitly toggles something or 30s passes.
// gcTime: 5min — keep cached data around during panel switches.
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      gcTime: 5 * 60_000,
      refetchOnWindowFocus: false,
      retry: 1,
    },
  },
});

// Detect which Tauri window is hosting this webview
const windowLabel = (() => {
  try {
    return getCurrentWindow().label ?? "main";
  } catch {
    return "main";
  }
})();

const root = ReactDOM.createRoot(document.getElementById("root") as HTMLElement);

if (!hasNativeBackend) {
  root.render(
    <main className="native-backend-required" role="alert" aria-labelledby="native-backend-required-title">
      <img src={brandLogo} alt="" />
      <p className="native-backend-required-kicker">WINCOMMANDER DEVELOPMENT</p>
      <h1 id="native-backend-required-title">Native backend disconnected</h1>
      <p>
        This browser tab can display the frontend shell, but it cannot read real Windows data or run Tauri commands.
      </p>
      <code>bun run dev:tauri:free</code>
      <p className="native-backend-required-note">
        Use the WinCommander desktop window opened by that command. Synthetic UI fixtures are isolated at
        <strong> /ui-audit.html</strong>.
      </p>
    </main>,
  );
} else if (windowLabel === "search-overlay") {
  // Mark html/body transparent so the Tauri transparent window actually shows through
  document.documentElement.classList.add("search-overlay-window");
  document.body.classList.add("search-overlay-window");

  // Dedicated transparent overlay window — render only the search bar
  root.render(
    <React.StrictMode>
      <GlobalErrorBoundary>
        <ThemeProvider>
          <EverythingSearchBar overlayMode />
        </ThemeProvider>
      </GlobalErrorBoundary>
    </React.StrictMode>
  );
} else if (windowLabel === "notification-alerts") {
  document.documentElement.classList.add("notification-toast-window");
  document.body.classList.add("notification-toast-window");

  root.render(
    <React.StrictMode>
      <GlobalErrorBoundary>
        <ThemeProvider>
          <CustomNotificationWindow />
        </ThemeProvider>
      </GlobalErrorBoundary>
    </React.StrictMode>
  );
} else {
  root.render(
    <React.StrictMode>
      <GlobalErrorBoundary>
        <QueryClientProvider client={queryClient}>
          <ThemeProvider>
            <AppConfirmProvider>
              <UsbHidApprovalProvider>
                <ExternalNotificationBridge />
                <App />
              </UsbHidApprovalProvider>
            </AppConfirmProvider>
          </ThemeProvider>
        </QueryClientProvider>
      </GlobalErrorBoundary>
    </React.StrictMode>
  );
}
