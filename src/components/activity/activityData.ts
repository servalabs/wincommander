// src/components/activity/activityData.ts
//
// Pure transforms behind the local Productivity panel's ActivityWatch viewers.
// AGPL-3.0, same as the rest of this repository — this is the Free app's own
// viewer layer for data that never leaves the machine unless the device is
// fleet-enrolled.
//
// Deliberately self-contained: no fleet-server wire types, no `@/lib/api`
// import, and no cross-repo coupling. The Pro fleet console has its own
// separate implementation of the same ideas governed by the WinCommander EULA;
// the two are NOT to be linked or synced, because `OPEN_CORE.md` puts fleet
// services on the proprietary side of the boundary and this file on the public
// side. Keeping them independent is the point, not an oversight.
//
// Everything here is a pure function of its arguments — no I/O, no React — so
// the panel's views stay about layout and these stay unit-testable.

export type ActivityItem = {
  id?: string;
  label: string;
  seconds: number;
};

export type ActivityTimelineEvent = {
  id?: string;
  /** Seconds since the device's own local midnight, 0..86400. */
  startSeconds: number;
  endSeconds: number;
  label: string;
  app: string;
  title: string;
  categoryPath: string[];
  color?: string;
};

export type ActivityCategory = {
  id: string;
  label: string;
  /** Total including descendants. */
  seconds: number;
  /** Time attributed to this node itself, excluding descendants. */
  directSeconds: number;
  color?: string;
  children: ActivityCategory[];
};

export type HourBucket = {
  hour: number;
  seconds: number;
  applications: ActivityItem[];
  titles: ActivityItem[];
};

const DAY_SECONDS = 24 * 60 * 60;

function safeSeconds(value: number): number {
  return Number.isFinite(value) ? Math.max(0, value) : 0;
}

export function formatActivityDuration(value: number): string {
  const seconds = Math.round(safeSeconds(value));
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const remaining = seconds % 60;
  const parts: string[] = [];
  if (hours) parts.push(`${hours}h`);
  if (hours || minutes) parts.push(`${minutes}m`);
  parts.push(`${remaining}s`);
  return parts.join(" ");
}

function rankedTotals(totals: Map<string, number>): ActivityItem[] {
  return [...totals.entries()]
    .map(([label, seconds]) => ({ label, seconds }))
    .sort((left, right) => right.seconds - left.seconds);
}

/** Fold events into 24 hour-of-day buckets, splitting an event that straddles
 *  an hour boundary across both by exact overlap rather than assigning it
 *  wholly to its start hour. */
export function bucketTimeline(events: ActivityTimelineEvent[]): HourBucket[] {
  return Array.from({ length: 24 }, (_, hour) => {
    const hourStart = hour * 3600;
    const hourEnd = hourStart + 3600;
    const applications = new Map<string, number>();
    const titles = new Map<string, number>();
    let seconds = 0;

    for (const event of events) {
      const start = Math.min(DAY_SECONDS, safeSeconds(event.startSeconds));
      const end = Math.min(DAY_SECONDS, safeSeconds(event.endSeconds));
      const overlap = Math.max(0, Math.min(end, hourEnd) - Math.max(start, hourStart));
      if (!overlap) continue;
      seconds += overlap;
      if (event.app) applications.set(event.app, (applications.get(event.app) ?? 0) + overlap);
      if (event.title) titles.set(event.title, (titles.get(event.title) ?? 0) + overlap);
    }

    return {
      hour,
      // An hour cannot contain more than an hour of activity even if
      // overlapping events sum past it.
      seconds: Math.min(3600, seconds),
      applications: rankedTotals(applications),
      titles: rankedTotals(titles),
    };
  });
}

type MutableCategory = ActivityCategory & { childMap: Map<string, MutableCategory> };

/** Path segments are joined with the ASCII unit separator, not the empty
 *  string: `id` is used as a React key, and joining with "" makes
 *  ["A","BC"] and ["AB","C"] collide on "ABC" — distinct subtrees sharing a
 *  key causes real reconciliation bugs. U+001F cannot occur in an app name or
 *  window title, so it is collision-free. */
const CATEGORY_ID_SEPARATOR = "\u001f";

function freezeCategory(category: MutableCategory): ActivityCategory {
  return {
    id: category.id,
    label: category.label,
    seconds: category.seconds,
    directSeconds: category.directSeconds,
    color: category.color,
    children: [...category.childMap.values()]
      .sort((left, right) => right.seconds - left.seconds)
      .map(freezeCategory),
  };
}

/** Build the nested category tree the sunburst and tree views render.
 *  `fallbackSeconds` covers the case where nothing was categorized at all —
 *  the chart still shows the day's total under "Uncategorized" rather than
 *  rendering empty and implying no activity. */
export function buildCategoryTree(
  events: ActivityTimelineEvent[],
  fallbackSeconds = 0,
): ActivityCategory[] {
  const entries = events.length > 0
    ? events.map((event) => ({
        path: event.categoryPath.length ? event.categoryPath : ["Uncategorized"],
        seconds: safeSeconds(event.endSeconds - event.startSeconds),
        color: event.color,
      }))
    : [{ path: ["Uncategorized"], seconds: safeSeconds(fallbackSeconds), color: "#CCC" }];

  const roots = new Map<string, MutableCategory>();
  for (const entry of entries) {
    if (!entry.seconds) continue;
    let siblings = roots;
    const ids: string[] = [];
    entry.path.forEach((label, index) => {
      ids.push(label);
      const id = ids.join(CATEGORY_ID_SEPARATOR);
      let node = siblings.get(label);
      if (!node) {
        node = {
          id,
          label,
          seconds: 0,
          directSeconds: 0,
          color: entry.color,
          children: [],
          childMap: new Map(),
        };
        siblings.set(label, node);
      }
      node.seconds += entry.seconds;
      node.color ??= entry.color;
      if (index === entry.path.length - 1) node.directSeconds += entry.seconds;
      siblings = node.childMap;
    });
  }

  return [...roots.values()]
    .sort((left, right) => right.seconds - left.seconds)
    .map(freezeCategory);
}

/** Flatten to a ranked list of leaf-attributed times, labelled by full path,
 *  for the "Top Categories" list. */
export function flattenCategoryActivity(
  categories: ActivityCategory[],
  ancestors: string[] = [],
): ActivityItem[] {
  return categories
    .flatMap((category) => {
      const path = [...ancestors, category.label];
      const direct = category.directSeconds > 0
        ? [{ id: category.id, label: path.join(" > "), seconds: category.directSeconds }]
        : [];
      return [...direct, ...flattenCategoryActivity(category.children, path)];
    })
    .sort((left, right) => right.seconds - left.seconds);
}

export function sumActivity(items: Array<{ seconds: number }>): number {
  return items.reduce((sum, item) => sum + safeSeconds(item.seconds), 0);
}
