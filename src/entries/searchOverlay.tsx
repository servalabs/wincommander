import React, { useEffect } from "react";
import ReactDOM from "react-dom/client";
import { emit } from "@tauri-apps/api/event";
import EverythingSearchBar from "../components/EverythingSearchBar";
import { ThemeProvider } from "../context/ThemeContext";
import GlobalErrorBoundary from "../components/ErrorBoundary";

function SearchOverlayRoot() {
  useEffect(() => {
    // The native window remains hidden until the search card has committed
    // and the browser has reached a paint frame. This prevents WebView2's
    // default dark surface from flashing on the first Ctrl+Space.
    const firstFrame = requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        void emit("search-overlay-ready");
      });
    });
    return () => cancelAnimationFrame(firstFrame);
  }, []);

  return <EverythingSearchBar overlayMode />;
}

export function mountSearchOverlay(): void {
  document.documentElement.classList.add("search-overlay-window");
  document.body.classList.add("search-overlay-window");
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <GlobalErrorBoundary>
        <ThemeProvider>
          <SearchOverlayRoot />
        </ThemeProvider>
      </GlobalErrorBoundary>
    </React.StrictMode>,
  );
}
