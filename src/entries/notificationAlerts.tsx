import React from "react";
import ReactDOM from "react-dom/client";
import CustomNotificationWindow from "../components/CustomNotificationWindow";
import { ThemeProvider } from "../context/ThemeContext";
import GlobalErrorBoundary from "../components/ErrorBoundary";

export function mountNotificationAlerts(): void {
  document.documentElement.classList.add("notification-toast-window");
  document.body.classList.add("notification-toast-window");
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <GlobalErrorBoundary>
        <ThemeProvider>
          <CustomNotificationWindow />
        </ThemeProvider>
      </GlobalErrorBoundary>
    </React.StrictMode>,
  );
}
