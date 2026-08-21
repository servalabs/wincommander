// src/components/AppShell.tsx
//
// AppShell — the main 3-column layout: TitleBar + (Sidebar | Content | RightSidebar).
// Extracted from App.tsx to reduce the god-component size.
//
// Props:
// src/components/AppShell.tsx
//
// AppShell — the main 3-column layout: TitleBar + (Sidebar | Content | RightSidebar).
// Extracted from App.tsx to reduce the god-component size.
//
// Props:
//   activePanel        — currently selected panel ID
//   onPanelChange      — callback when user clicks a navigation item

import { ReactNode, useRef, useEffect } from "react";
import TitleBar from "./TitleBar";
import Sidebar from "./Sidebar";
import RightSidebar from "./RightSidebar";
import GlobalCommandPalette from "./GlobalCommandPalette";
import type { PanelId } from "../types/panels";
import { useDashboardScale } from "../hooks/useWindowScale";
import { applyMotionClass } from "../lib/motionPolicy";
import { useSearchQuery } from "../context/SearchContext";

interface AppShellProps {
  activePanel: PanelId;
  onPanelChange: (panel: PanelId) => void;
  /** Called on intentional sidebar hover to silently pre-fetch panel data */
  onPanelHover?: (panel: PanelId) => void;
  showUnlockedPanels: boolean;
  children: ReactNode;
}

export default function AppShell({
  activePanel,
  onPanelChange,
  onPanelHover,
  showUnlockedPanels,
  children,
}: AppShellProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const contentScrollRef = useRef<HTMLDivElement>(null);
  const previousPanelRef = useRef<PanelId>(activePanel);
  const scale = useDashboardScale(containerRef);
  const isDashboard = activePanel === "dashboard";
  const isViewportBoundPanel = activePanel === "search-files" || activePanel === "cleanup";
  const { clearSearch } = useSearchQuery();

  // Reset any palette-seeded panel filter whenever the user switches panels.
  useEffect(() => { clearSearch(); }, [activePanel, clearSearch]);

  // Normal panel navigation should land at the top of the new panel. Direct
  // section jumps fire their own scroll event after mount and will override this.
  useEffect(() => {
    const previousPanel = previousPanelRef.current;
    previousPanelRef.current = activePanel;
    if (previousPanel === activePanel || isDashboard) return;
    requestAnimationFrame(() => {
      contentScrollRef.current?.scrollTo({ top: 0, left: 0, behavior: "auto" });
    });
  }, [activePanel, isDashboard]);

  useEffect(() => {
    const scrollTop = () => {
      contentScrollRef.current?.scrollTo({ top: 0, left: 0, behavior: "smooth" });
    };
    window.addEventListener("panel-scroll-top", scrollTop);
    return () => window.removeEventListener("panel-scroll-top", scrollTop);
  }, []);

  // Re-apply the motion preference on mount. main.tsx applies it before first
  // render (so the splash honors it); this re-run keeps the class correct after
  // any remount. Logic (explicit choice → OS preference → low-spec default)
  // lives in motionPolicy so AppShell and startup can't drift apart.
  useEffect(() => {
    applyMotionClass();
  }, []);

  return (
    <>
      <TitleBar activePanel={activePanel} />
      <div className="app-body flex overflow-hidden">
        {/* Column 1: Left Navigation */}
        <Sidebar
          activePanel={activePanel}
          onPanelChange={onPanelChange}
          onPanelHover={onPanelHover}
          showUnlockedPanels={showUnlockedPanels}
        />

        {/* Column 2: Main Content Stage with Global Scaling */}
        <main
          className={`app-content flex-1 flex flex-col h-full relative ${isDashboard ? 'p-0' : ''}`}
          ref={containerRef}
          style={{ overflow: 'hidden' }}
        >
          {/* Per-panel search bar removed — discovery + the unlock/lock keyword
              now live in the universal ⌘K command palette. */}
          {isDashboard ? (
            /* Dashboard: scale the entire content area to fit the window.
               Use absolute positioning so flex-shrink cannot reduce the
               element below the intended 100%/scale height — which would
               cause the transform to show at <100% and leave blank space. */
            <div
              style={{
                position: 'absolute',
                top: 0,
                left: 0,
                width: `calc(100% / ${scale})`,
                height: `calc(100% / ${scale})`,
                transform: `scale(${scale})`,
                transformOrigin: 'top left',
              }}
            >
              {children}
            </div>
          ) : (
            /* Viewport-bound panels own their scroll lifecycle; let them fill
               the stage instead of nesting inside AppShell page scroll. */
            <div
              className={`app-panel-stage flex-1 min-h-0 overflow-x-hidden custom-scrollbar relative ${isViewportBoundPanel ? "overflow-hidden" : "overflow-y-auto"}`}
              ref={contentScrollRef}
              style={{ paddingBottom: isViewportBoundPanel ? 0 : 56 }}
            >
              {children}
            </div>
          )}
        </main>

        {/* Column 3: Right Quick Actions */}
        <RightSidebar />
      </div>

      {/* V2 global overlays — self-managed via window events */}
      <GlobalCommandPalette />
    </>
  );
}


