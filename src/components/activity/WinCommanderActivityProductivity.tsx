// src/components/activity/WinCommanderActivityProductivity.tsx
//
// Composes the local Productivity panel's activity viewers into one day view.
// AGPL-3.0, part of the Free app's own viewer layer.
//
// The Pro fleet console has a separate, richer composition under the
// WinCommander EULA. Do NOT sync or diff the two: `OPEN_CORE.md` places fleet
// services on the proprietary side of the boundary and this file on the public
// side, so the two are independent by design.
//
// Data comes from the local ActivityWatch REST API (src/lib/activityWatch.ts),
// so this view is scoped to ONE day at a time. That scope is why there is no
// per-app drilldown, input-activity chart or raw-bucket explorer here: each of
// those needs multi-day history, and a prop with no local data source to fill
// it would be permanently empty rather than useful.

import { ActivityCategoryCharts } from "./ActivityCategoryCharts";
import { ActivityRankedList } from "./ActivityRankedList";
import { ActivityTimelineChart } from "./ActivityTimelineChart";
import { TimelineSwimlane } from "./TimelineSwimlane";
import {
  formatActivityDuration,
  sumActivity,
  type ActivityCategory,
  type ActivityItem,
  type ActivityTimelineEvent,
} from "./activityData";
import "./winCommanderActivityProductivity.css";

export type WinCommanderActivityProductivityProps = {
  deviceName: string;
  dateLabel: string;
  applications: ActivityItem[];
  windowTitles: ActivityItem[];
  timelineEvents: ActivityTimelineEvent[];
  categories: ActivityCategory[];
  timezoneLabel?: string;
  emptyMessage?: string;
};

export function WinCommanderActivityProductivity({
  deviceName,
  dateLabel,
  applications,
  windowTitles,
  timelineEvents,
  categories,
  timezoneLabel = "device local time",
  emptyMessage = "No activity was reported for this device and date.",
}: WinCommanderActivityProductivityProps) {
  const activeSeconds = sumActivity(applications);

  return (
    <article className="wc-fleet-productivity" aria-label={`${deviceName} productivity for ${dateLabel}`}>
      <header className="wc-fleet-productivity-head">
        <div>
          <p>Activity for <strong>{dateLabel}</strong></p>
          <h2>{deviceName}</h2>
          <span>Time active: {formatActivityDuration(activeSeconds)}</span>
        </div>
      </header>
      <div className="wc-fleet-activity-grid">
        <ActivityRankedList title="Top Applications" items={applications} emptyMessage={emptyMessage} />
        <ActivityRankedList title="Top Window Titles" items={windowTitles} emptyMessage={emptyMessage} />
        <ActivityTimelineChart events={timelineEvents} timezoneLabel={timezoneLabel} emptyMessage={emptyMessage} />
      </div>
      <ActivityCategoryCharts categories={categories} emptyMessage={emptyMessage} />

      <div className="wc-fleet-category-section">
        <TimelineSwimlane events={timelineEvents} onSelectApp={() => {}} />
      </div>
    </article>
  );
}
