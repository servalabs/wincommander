// src/hooks/useActivityWatchDay.ts
//
// Orchestrates a single day's worth of ActivityWatch data for the
// Productivity panel: bucket discovery, window/AFK intersection, category
// classification (all in src/lib/activityWatch.ts), plus the extra bucket types
// (src/lib/activityWatchExtras.ts). Degrades explicitly through three
// distinct non-happy states — AW unreachable, no buckets recorded yet, and
// an empty day — so the panel never shows an endless spinner or a crash.

import { useEffect, useState } from "react";
import { ensureActivityWatchStarted } from "./activityWatchIpc";
import {
  activeIntervals,
  AW_BUCKET_FETCH_CONCURRENCY,
  clipEventInterval,
  dayBoundsLocal,
  eventData,
  fetchBucketEvents,
  fetchBuckets,
  fetchServerInfo,
  intersectTimeline,
  loadCategories,
  mapWithConcurrency,
  sanitizeWindowEvent,
  selectActivityBuckets,
  summarizeByField,
  toActivityTimelineEvents,
  type AwBucketInfo,
  type AwBucketsMap,
  type AwEvent,
  type RawTimelineEvent,
} from "@/lib/activityWatch";
import {
  classifyBucketId,
  summarizeGenericBucket,
  summarizeInputEvents,
  summarizeVscodeEvents,
  summarizeWebEvents,
  type GenericBucketSummary,
  type InputSummary,
  type VscodeSummary,
  type WebSummary,
} from "@/lib/activityWatchExtras";
import { buildCategoryTree, type ActivityCategory, type ActivityItem, type ActivityTimelineEvent } from "@/components/activity/activityData";

export type ActivitySource = "window" | "web" | "vscode";

/** One chronological entry across ALL bucket types, for the native
 * Timeline/Search tabs (which replace ActivityWatch's own timeline + query
 * pages). `detail` is always plain text — never rendered as a link. */
export interface CombinedActivityEvent {
  timestampMs: number;
  duration: number;
  source: ActivitySource;
  title: string;
  detail: string;
  categoryPath?: string[];
  color?: string;
}

export interface ActivityDayData {
  dateLabel: string;
  deviceName: string;
  timezoneLabel: string;
  applications: ActivityItem[];
  windowTitles: ActivityItem[];
  timelineEvents: ActivityTimelineEvent[];
  categories: ActivityCategory[];
  combinedEvents: CombinedActivityEvent[];
  web: WebSummary | null;
  vscode: VscodeSummary | null;
  input: InputSummary | null;
  generic: GenericBucketSummary[];
}

export type ActivityWatchDayState =
  | { status: "loading" }
  | { status: "unavailable"; message: string }
  | { status: "no-buckets" }
  | { status: "empty"; deviceName: string; dateLabel: string }
  | { status: "ready"; day: ActivityDayData };

/** Buckets with no hostname recorded are assumed local — most non-window
 * ActivityWatch watchers (web/vscode/input) don't stamp one on a
 * single-machine install. Only an EXPLICIT mismatch excludes a bucket, so a
 * synced multi-device AW database never mixes another device's browsing or
 * coding activity into this one's report. */
function belongsToHost(bucketId: string, bucket: AwBucketInfo, hostname: string): boolean {
  if (bucket.hostname?.trim()) return bucket.hostname.toLowerCase() === hostname.toLowerCase();
  // Metadata-less watcher buckets must still carry the local machine suffix.
  // An unscoped bucket is deliberately excluded rather than attributed to the
  // current device when an ActivityWatch store has been synchronised.
  return bucketId.toLowerCase().endsWith(`_${hostname.toLowerCase()}`);
}

function formatDateLabel(date: Date): string {
  return date.toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric", year: "numeric" });
}

function bucketIdsOfKind(buckets: AwBucketsMap, kind: string, hostname: string): string[] {
  return Object.entries(buckets)
    .filter(([id, info]) => classifyBucketId(id) === kind && belongsToHost(id, info, hostname))
    .map(([id]) => id);
}

function buildCombinedEvents(
  windowEvents: RawTimelineEvent[],
  webEvents: AwEvent[],
  vscodeEvents: AwEvent[],
  dayStartMs: number,
  dayEndMs: number,
): CombinedActivityEvent[] {
  const combined: CombinedActivityEvent[] = windowEvents.map((event) => ({
    timestampMs: event.timestampMs,
    duration: event.duration,
    source: "window",
    title: event.title || event.app || "Unknown window",
    detail: event.app,
    categoryPath: event.categoryPath,
    color: event.categoryColor,
  }));

  for (const event of webEvents) {
    const interval = clipEventInterval(event, dayStartMs, dayEndMs);
    if (!interval) continue;
    const data = eventData(event.data);
    const url = typeof data.url === "string" ? data.url : "";
    const title = typeof data.title === "string" ? data.title : "";
    if (!url && !title) continue;
    combined.push({
      timestampMs: interval.startMs,
      duration: (interval.endMs - interval.startMs) / 1000,
      source: "web",
      title: title || url,
      detail: url,
    });
  }

  for (const event of vscodeEvents) {
    const interval = clipEventInterval(event, dayStartMs, dayEndMs);
    if (!interval) continue;
    const data = eventData(event.data);
    const file = typeof data.file === "string" ? data.file : "";
    const project = typeof data.project === "string" ? data.project : "";
    const language = typeof data.language === "string" ? data.language : "";
    if (!file && !project) continue;
    combined.push({
      timestampMs: interval.startMs,
      duration: (interval.endMs - interval.startMs) / 1000,
      source: "vscode",
      title: file || project,
      detail: [project, language].filter(Boolean).join(" · "),
    });
  }

  return combined.sort((a, b) => a.timestampMs - b.timestampMs);
}

// `date` must be a stable reference across renders unless the caller
// actually changed the selected day (e.g. from useState) — the effect
// below intentionally depends on it directly rather than a re-derived key,
// so a fresh `new Date()` recreated every render would cause a refetch loop.
export function useActivityWatchDay(date: Date, hostname: string | null): ActivityWatchDayState {
  const [state, setState] = useState<ActivityWatchDayState>({ status: "loading" });
  const localHostname = hostname && hostname.trim() ? hostname : null;

  useEffect(() => {
    let cancelled = false;
    const controller = new AbortController();
    setState({ status: "loading" });

    (async () => {
      try {
        await ensureActivityWatchStarted();
      } catch {
        if (!cancelled) {
          setState({
            status: "unavailable",
            message: "ActivityWatch could not be started or reached. Check Productivity settings, then retry.",
          });
        }
        return;
      }
      const bounds = dayBoundsLocal(date);
      const dateLabel = formatDateLabel(date);
      const deviceName = localHostname ?? "This device";

      let buckets: AwBucketsMap;
      try {
        buckets = await fetchBuckets(controller.signal);
      } catch {
        if (!cancelled) {
          setState({
            status: "unavailable",
            message: "ActivityWatch is not installed or not running. Install it from activitywatch.net, or start it, to see activity here.",
          });
        }
        return;
      }
      if (cancelled) return;
      if (Object.keys(buckets).length === 0) {
        setState({ status: "no-buckets" });
        return;
      }

      try {
        const [categories, serverInfo] = await Promise.all([loadCategories(controller.signal), fetchServerInfo(controller.signal)]);
        const activityHostname = localHostname ?? (serverInfo.hostname?.trim() || null);
        if (!activityHostname) {
          if (!cancelled) {
            setState({
              status: "unavailable",
              message: "ActivityWatch did not identify its local device, so this app will not show potentially synced activity from another machine.",
            });
          }
          return;
        }
        const selected = selectActivityBuckets(buckets, activityHostname);

        let timelineEvents: RawTimelineEvent[] = [];
        if (selected) {
          const windowEvents = await fetchBucketEvents(selected.windowId, bounds.startIso, bounds.endIso, -1, controller.signal);
          const sanitized = windowEvents
            .map((event) => sanitizeWindowEvent(event, bounds.startMs, bounds.endMs, categories))
            .filter((event): event is RawTimelineEvent => event !== null);

          const afkEvents = selected.afkId
            ? await fetchBucketEvents(selected.afkId, bounds.startIso, bounds.endIso, -1, controller.signal).catch(() => [] as AwEvent[])
            : [];
          const active = selected.afkId ? activeIntervals(afkEvents, bounds.startMs, bounds.endMs) : null;
          // null => no usable AFK state, fall back to raw window events.
          // [] (even after intersection) => AFK explicitly says never
          // active — must NOT fall back (see activeIntervals doc comment).
          timelineEvents = active === null ? sanitized : intersectTimeline(sanitized, active);
        }

        const webIds = bucketIdsOfKind(buckets, "web", activityHostname);
        const vscodeIds = bucketIdsOfKind(buckets, "vscode", activityHostname);
        const inputIds = bucketIdsOfKind(buckets, "input", activityHostname);
        const genericIds = bucketIdsOfKind(buckets, "generic", activityHostname);

        const requests = [
          ...webIds.map((id) => ({ id, source: "web" as const })),
          ...vscodeIds.map((id) => ({ id, source: "vscode" as const })),
          ...inputIds.map((id) => ({ id, source: "input" as const })),
          ...genericIds.map((id) => ({ id, source: "generic" as const })),
        ];
        const fetched = await mapWithConcurrency(requests, AW_BUCKET_FETCH_CONCURRENCY, async (request) => ({
          ...request,
          events: await fetchBucketEvents(request.id, bounds.startIso, bounds.endIso, -1, controller.signal)
            .catch(() => [] as AwEvent[]),
        }), controller.signal);
        if (cancelled) return;
        const eventsById = new Map(fetched.map((result) => [result.id, result.events]));
        const webEvents = webIds.flatMap((id) => eventsById.get(id) ?? []);
        const vscodeEvents = vscodeIds.flatMap((id) => eventsById.get(id) ?? []);
        const inputEvents = inputIds.flatMap((id) => eventsById.get(id) ?? []);

        const generic: GenericBucketSummary[] = [];
        for (const id of genericIds) {
          const events = eventsById.get(id) ?? [];
          if (events.length > 0) generic.push(summarizeGenericBucket(id, buckets[id]?.type ?? "unknown", events));
        }

        const web = webIds.length > 0 ? summarizeWebEvents(webEvents) : null;
        const vscode = vscodeIds.length > 0 ? summarizeVscodeEvents(vscodeEvents) : null;
        const input = inputIds.length > 0 ? summarizeInputEvents(inputEvents) : null;

        const timelineForChart = toActivityTimelineEvents(timelineEvents);
        const hasAnyData =
          timelineEvents.length > 0 ||
          (web !== null && (web.topUrls.length > 0 || web.topTitles.length > 0)) ||
          (vscode !== null && (vscode.topProjects.length > 0 || vscode.topFiles.length > 0)) ||
          (input !== null && input.presses + input.clicks + input.scrolls > 0) ||
          generic.length > 0;

        if (cancelled) return;
        if (!hasAnyData) {
          setState({ status: "empty", deviceName, dateLabel });
          return;
        }

        setState({
          status: "ready",
          day: {
            dateLabel,
            deviceName,
            timezoneLabel: "device local time",
            applications: summarizeByField(timelineEvents, "app"),
            windowTitles: summarizeByField(timelineEvents, "title"),
            timelineEvents: timelineForChart,
            categories: buildCategoryTree(timelineForChart),
            combinedEvents: buildCombinedEvents(timelineEvents, webEvents, vscodeEvents, bounds.startMs, bounds.endMs),
            web,
            vscode,
            input,
            generic,
          },
        });
      } catch {
        if (!cancelled) {
          setState({
            status: "unavailable",
            message: "Couldn't read ActivityWatch data for this device — the local server stopped responding.",
          });
        }
      }
    })();

    return () => {
      cancelled = true;
      controller.abort();
    };
  }, [date, localHostname]);

  return state;
}
