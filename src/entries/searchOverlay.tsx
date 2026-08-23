import React from "react";
import ReactDOM from "react-dom/client";
import EverythingSearchBar from "../components/EverythingSearchBar";
import { ThemeProvider } from "../context/ThemeContext";
import GlobalErrorBoundary from "../components/ErrorBoundary";

export function mountSearchOverlay(): void {
  document.documentElement.classList.add("search-overlay-window");
  document.body.classList.add("search-overlay-window");
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <GlobalErrorBoundary>
        <ThemeProvider>
          <EverythingSearchBar overlayMode />
        </ThemeProvider>
      </GlobalErrorBoundary>
    </React.StrictMode>,
  );
}
