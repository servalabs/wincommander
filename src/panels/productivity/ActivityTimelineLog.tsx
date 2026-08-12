// src/panels/productivity/ActivityTimelineLog.tsx
//
// "Timeline" tab body: a chronological, source-tagged log of every
// ActivityWatch event for the selected day (window focus + browser + VS
// Code) — replaces ActivityWatch's own /#/timeline page. Pure renderer;
// the Search tab (ActivitySearchView.tsx) reuses this with a filtered
// `events` array.

import { Icon, type IconName } from "@/components/ui/bp";
import { formatActivityDuration } from "@/components/activity/activityData";
import type { CombinedActivityEvent } from "@/hooks/useActivityWatchDay";

interface ActivityTimelineLogProps {
  events: CombinedActivityEvent[];
  emptyMessage: string;
}

const SOURCE_ICON: Record<CombinedActivityEvent["source"], IconName> = {
  window: "desktop",
  web: "globe-network",
  vscode: "code",
};

const SOURCE_LABEL: Record<CombinedActivityEvent["source"], string> = {
  window: "App",
  web: "Web",
  vscode: "VS Code",
};

function formatTime(ms: number): string {
  return new Date(ms).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

export default function ActivityTimelineLog({ events, emptyMessage }: ActivityTimelineLogProps) {
  if (events.length === 0) {
    return <p className="productivity-timeline-empty">{emptyMessage}</p>;
  }

  return (
    <ol className="productivity-timeline-log" aria-label="Chronological activity log">
      {events.map((event, index) => (
        <li key={`${event.timestampMs}-${index}`} className="productivity-timeline-row">
          <span className="productivity-timeline-time">{formatTime(event.timestampMs)}</span>
          <span className="productivity-timeline-source" title={SOURCE_LABEL[event.source]}>
            <Icon icon={SOURCE_ICON[event.source]} size={13} />
          </span>
          <span className="productivity-timeline-body">
            <span className="productivity-timeline-title">{event.title}</span>
            {event.detail && <span className="productivity-timeline-detail">{event.detail}</span>}
          </span>
          {event.categoryPath && event.categoryPath.length > 0 && (
            <span className="productivity-timeline-category" style={event.color ? { color: event.color } : undefined}>
              {event.categoryPath.join(" › ")}
            </span>
          )}
          <span className="productivity-timeline-duration">{formatActivityDuration(event.duration)}</span>
        </li>
      ))}
    </ol>
  );
}
