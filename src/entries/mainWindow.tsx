import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import App from "../App";
import { ThemeProvider } from "../context/ThemeContext";
import ExternalNotificationBridge from "../components/ExternalNotificationBridge";
import GlobalErrorBoundary from "../components/ErrorBoundary";
import { AppConfirmProvider } from "../components/shared/AppConfirmDialog";
import { UsbHidApprovalProvider } from "../context/UsbHidApprovalContext";
import { initUniversalLogging } from "../lib/logger";
import { reportStartupPhase } from "../hooks/startupTrace";

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

export function mountMainWindow(): void {
  reportStartupPhase("webview_dom_ready");
  initUniversalLogging();
  window.addEventListener(
    "keydown",
    (event) => {
      if (event.key === "F12" && !event.ctrlKey && !event.shiftKey && !event.altKey && !event.metaKey) {
        event.preventDefault();
        event.stopImmediatePropagation();
      }
    },
    true,
  );

  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
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
    </React.StrictMode>,
  );
}
