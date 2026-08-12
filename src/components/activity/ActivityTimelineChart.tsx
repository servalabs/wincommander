// src/components/activity/ActivityTimelineChart.tsx
//
// Hour-of-day activity bar chart for the local Productivity panel. AGPL-3.0,
// part of the Free app's own viewer layer.
//
// The Pro fleet console has a separate implementation under the WinCommander
// EULA. Do NOT sync or diff the two: `OPEN_CORE.md` places fleet services on
// the proprietary side of the boundary and this file on the public side.

import { useMemo, useState } from "react";
import { bucketTimeline, formatActivityDuration, type ActivityTimelineEvent } from "./activityData";

type Props = {
  events: ActivityTimelineEvent[];
  timezoneLabel: string;
  emptyMessage: string;
};

const HOUR_SECONDS = 3600;
const Y_TICKS = [HOUR_SECONDS, 2700, 1800, 900, 0];

function hourLabel(hour: number): string {
  return `${String(hour).padStart(2, "0")}:00–${String((hour + 1) % 24).padStart(2, "0")}:00`;
}

export function ActivityTimelineChart({ events, timezoneLabel, emptyMessage }: Props) {
  const buckets = useMemo(() => bucketTimeline(events), [events]);
  const [selectedHour, setSelectedHour] = useState<number | null>(null);
  const [hoveredHour, setHoveredHour] = useState<number | null>(null);
  const activeHour = hoveredHour ?? selectedHour;
  const activeBucket = activeHour === null ? null : buckets[activeHour];
  const hasData = buckets.some((bucket) => bucket.seconds > 0);

  return (
    <section className="wc-fleet-section wc-fleet-timeline" aria-label="Timeline barchart">
      <h3>Timeline (barchart)</h3>
      <div className="wc-fleet-chart" aria-label={`Activity timeline in ${timezoneLabel}`}>
        <div className="wc-fleet-y-axis" aria-hidden="true">
          {Y_TICKS.map((tick) => <span key={tick}>{formatActivityDuration(tick)}</span>)}
        </div>
        <div className="wc-fleet-plot">
          <div className="wc-fleet-grid-lines" aria-hidden="true" />
          <div className="wc-fleet-bars">
            {buckets.map((bucket) => (
              <button
                type="button"
                key={bucket.hour}
                className={selectedHour === bucket.hour ? "is-selected" : ""}
                aria-label={`${hourLabel(bucket.hour)}: ${formatActivityDuration(bucket.seconds)}`}
                aria-pressed={selectedHour === bucket.hour}
                onClick={() => setSelectedHour((value) => value === bucket.hour ? null : bucket.hour)}
                onMouseEnter={() => setHoveredHour(bucket.hour)}
                onMouseLeave={() => setHoveredHour(null)}
                onFocus={() => setHoveredHour(bucket.hour)}
                onBlur={() => setHoveredHour(null)}
              >
                <i style={{ height: `${bucket.seconds / HOUR_SECONDS * 100}%` }} />
              </button>
            ))}
          </div>
          <div className="wc-fleet-x-axis" aria-hidden="true">
            {buckets.map(({ hour }) => <span key={hour}>{hour % 3 === 1 ? hour : ""}</span>)}
          </div>
          {!hasData && <p className="wc-fleet-chart-empty">{emptyMessage}</p>}
        </div>
      </div>
      <div className="wc-fleet-chart-foot">
        <span>Time of day ({timezoneLabel})</span>
        {activeBucket && (
          <output aria-live="polite">
            <b>{hourLabel(activeBucket.hour)}</b>
            <span>{formatActivityDuration(activeBucket.seconds)}</span>
            {activeBucket.applications[0] && <span>{activeBucket.applications[0].label}</span>}
          </output>
        )}
      </div>
    </section>
  );
}
