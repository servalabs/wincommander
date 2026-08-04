// ══════════════════════════════════════════════════════════════════════════
// Productivity Panel — native ActivityWatch viewer
// ══════════════════════════════════════════════════════════════════════════
// Reads ActivityWatch's local REST API (http://localhost:5600) directly and
// renders with this app's own UI kit, instead of embedding AW's Vue web UI
// in a WebView2 and CSS-hacking its navbar/header/footer away. That embed
// broke whenever AW changed markup and needed webview lifecycle workarounds
// (hide_all_server_apps on unmount, remount-by-hostname keys) — gone now.
//
// Data + domain logic: src/lib/activityWatch.ts (window/AFK/category
// semantics, mirrors commander-pro/src/productivity_detail.rs),
// src/lib/activityWatchExtras.ts (browser/VS Code/input/generic buckets),
// src/hooks/useActivityWatchDay.ts (orchestration + explicit non-happy
// states). Presentational components: src/components/activity/ (canonical
// copies of the Fleet console's productivity views).

import { useEffect, useState } from "react";
import { H5, Icon, NonIdealState, Spinner, Text } from "@/components/ui/bp";
import { WinCommanderActivityProductivity } from "@/components/activity/WinCommanderActivityProductivity";
import { useActivityWatchDay } from "@/hooks/useActivityWatchDay";
import { getFleetStatus } from "@/hooks/fleetStatus";
import { useAppState } from "../../context/AppContext";
import { open } from "@tauri-apps/plugin-shell";
import DateNav from "./DateNav";
import ActivityTimelineLog from "./ActivityTimelineLog";
import ActivitySearchView from "./ActivitySearchView";
import ExtraSourcesSection from "./ExtraSourcesSection";
import "./index.css";

type ProductivityView = "activity" | "timeline" | "search";

const VIEW_TABS: Array<{ id: ProductivityView; label: string; icon: "dashboard" | "time" | "search" }> = [
  { id: "activity", label: "Activity", icon: "dashboard" },
  { id: "timeline", label: "Timeline", icon: "time" },
  { id: "search", label: "Search", icon: "search" },
];

function startOfToday(): Date {
  const date = new Date();
  date.setHours(0, 0, 0, 0);
  return date;
}

/** Task 2c: this used to unconditionally claim "never uploaded", which was
 * false the moment a device is fleet-enrolled — the fleet agent's detail
 * collector reads this same ActivityWatch instance independently of any
 * consent toggle. State the truth for whichever mode this device is in. */
function useProductivitySubtitle(): string {
  const { appSettings } = useAppState();
  const fleetEnabledSetting = appSettings?.app?.fleet?.enabled === true;
  const [fleetConnected, setFleetConnected] = useState(false);

  useEffect(() => {
    if (!fleetEnabledSetting) {
      setFleetConnected(false);
      return;
    }
    let cancelled = false;
    getFleetStatus()
      .then((status) => { if (!cancelled) setFleetConnected(status.connected); })
      .catch(() => { if (!cancelled) setFleetConnected(false); });
    return () => { cancelled = true; };
  }, [fleetEnabledSetting]);

  return fleetEnabledSetting && fleetConnected
    ? "This device is fleet-enrolled: app names, window titles, URLs, file paths, and activity are reported to the fleet."
    : "Data stays on this device — not fleet-enrolled, so nothing here is uploaded.";
}

export default function ProductivityPanel() {
  const [activeView, setActiveView] = useState<ProductivityView>("activity");
  const [selectedDate, setSelectedDate] = useState<Date>(() => startOfToday());
  const { systemInfo } = useAppState();
  const subtitle = useProductivitySubtitle();

  const hostname = systemInfo?.hostname || null;
  const state = useActivityWatchDay(selectedDate, hostname);

  return (
    <div className="panel-container productivity-panel">
      <div className="productivity-header">
        <div>
          <H5 className="header-title">Productivity</H5>
          <Text className="header-subtext">{subtitle}</Text>
        </div>

        {/* Browser & IDE Extension links */}
        <div className="productivity-watchers">
          <span className="watchers-label">WATCHERS</span>
          <div className="watcher-links">
            <button type="button" onClick={() => open('https://chromewebstore.google.com/detail/activitywatch-web-watcher/nglaklhklhcoonedhgnpgddginnjdadi')} className="watcher-badge" title="Chrome Extension">
              <Icon icon="globe-network" size={12} /> Chrome
            </button>
            <button type="button" onClick={() => open('https://addons.mozilla.org/en-US/firefox/addon/aw-watcher-web/')} className="watcher-badge" title="Firefox Add-on">
              <Icon icon="globe-network" size={12} /> Firefox
            </button>
            <button type="button" onClick={() => open('https://marketplace.visualstudio.com/items?itemName=activitywatch.aw-watcher-vscode')} className="watcher-badge" title="VS Code Extension">
              <Icon icon="code" size={12} /> VS Code
            </button>
          </div>
        </div>
      </div>

      <div className="productivity-toolbar">
        <div className="productivity-view-tabs">
          {VIEW_TABS.map((v) => (
            <button
              key={v.id}
              type="button"
              aria-pressed={activeView === v.id}
              className={`productivity-view-tab ${activeView === v.id ? "active" : ""}`}
              onClick={() => activeView !== v.id && setActiveView(v.id)}
            >
              <Icon icon={v.icon} size={14} />
              <span className="tab-label">{v.label}</span>
            </button>
          ))}
        </div>
        <DateNav date={selectedDate} onChange={setSelectedDate} />
      </div>

      <div className="productivity-body">
        {state.status === "loading" && (
          <div className="productivity-loading">
            <Spinner size={24} />
            <Text>Reading ActivityWatch…</Text>
          </div>
        )}

        {state.status === "unavailable" && (
          <NonIdealState
            icon="offline"
            title="ActivityWatch isn't reachable"
            description={state.message}
          />
        )}

        {state.status === "no-buckets" && (
          <NonIdealState
            icon="history"
            title="No activity recorded yet"
            description="ActivityWatch is running but hasn't recorded any buckets yet. Give it a minute after startup, or install the browser/VS Code watchers above for fuller coverage."
          />
        )}

        {state.status === "empty" && (
          <NonIdealState
            icon="calendar"
            title={`No activity for ${state.dateLabel}`}
            description={`Nothing was recorded for ${state.deviceName} on this date.`}
          />
        )}

        {state.status === "ready" && activeView === "activity" && (
          <>
            <WinCommanderActivityProductivity
              deviceName={state.day.deviceName}
              dateLabel={state.day.dateLabel}
              applications={state.day.applications}
              windowTitles={state.day.windowTitles}
              timelineEvents={state.day.timelineEvents}
              categories={state.day.categories}
              timezoneLabel={state.day.timezoneLabel}
            />
            <ExtraSourcesSection
              web={state.day.web}
              vscode={state.day.vscode}
              input={state.day.input}
              generic={state.day.generic}
              emptyMessage={`No activity was reported for this device and date.`}
            />
          </>
        )}

        {state.status === "ready" && activeView === "timeline" && (
          <ActivityTimelineLog
            events={state.day.combinedEvents}
            emptyMessage={`No activity recorded for ${state.day.dateLabel}.`}
          />
        )}

        {state.status === "ready" && activeView === "search" && (
          <ActivitySearchView events={state.day.combinedEvents} />
        )}
      </div>
    </div>
  );
}
