// src/lib/activityWatchExtras.ts
//
// Aggregation for ActivityWatch bucket types beyond window/AFK (the pair
// productivity_detail.rs and src/lib/activityWatch.ts already cover).
// The owner wants full-fidelity local collection, so this reads whatever
// aw-watcher-web / aw-watcher-vscode / aw-watcher-input buckets exist, plus
// a generic fallback for any other bucket type: full URLs, page titles,
// project/file/language, and aggregate input counts. aw-watcher-input
// exposes counts only (presses/clicks/pointer movement) — ActivityWatch has
// no keystroke-content field, so there is nothing here to invent a keylogger
// from.

import type { ActivityItem } from "@/components/activity/activityData";
import type { AwEvent } from "./activityWatch";

function isFiniteNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

function stringField(data: Record<string, unknown>, key: string): string {
  const value = data[key];
  return typeof value === "string" ? value : "";
}

function toRanked(totals: Map<string, number>): ActivityItem[] {
  return [...totals.entries()]
    .map(([label, seconds]) => ({ label, seconds }))
    .sort((a, b) => b.seconds - a.seconds);
}

function summarizeByFields(events: AwEvent[], fields: string[]): ActivityItem[][] {
  const totals = fields.map(() => new Map<string, number>());
  for (const event of events) {
    if (!Number.isFinite(event.duration) || event.duration <= 0) continue;
    fields.forEach((field, index) => {
      const value = stringField(event.data ?? {}, field);
      if (!value) return;
      totals[index].set(value, (totals[index].get(value) ?? 0) + event.duration);
    });
  }
  return totals.map(toRanked);
}

// ── aw-watcher-web-* (browser extension) ─────────────────────────────────

export interface WebSummary {
  topUrls: ActivityItem[];
  topTitles: ActivityItem[];
}

/** Full URLs and page titles — the owner wants full local fidelity, no
 * hashing/truncation of what the browser watcher already reports. */
export function summarizeWebEvents(events: AwEvent[]): WebSummary {
  const [topUrls, topTitles] = summarizeByFields(events, ["url", "title"]);
  return { topUrls, topTitles };
}

// ── aw-watcher-vscode_* ───────────────────────────────────────────────────

export interface VscodeSummary {
  topProjects: ActivityItem[];
  topFiles: ActivityItem[];
  topLanguages: ActivityItem[];
}

export function summarizeVscodeEvents(events: AwEvent[]): VscodeSummary {
  const [topProjects, topFiles, topLanguages] = summarizeByFields(events, ["project", "file", "language"]);
  return { topProjects, topFiles, topLanguages };
}

// ── aw-watcher-input_* ────────────────────────────────────────────────────

export interface InputSummary {
  presses: number;
  clicks: number;
  /** Some input-watcher forks report scroll events; 0 when the field is absent. */
  scrolls: number;
  /** Cumulative mouse-movement distance in pixels (deltaX/deltaY summed). */
  movementPx: number;
}

/** Aggregate counts only — never keystroke content (ActivityWatch's input
 * watcher does not capture what was typed, only how much). */
export function summarizeInputEvents(events: AwEvent[]): InputSummary {
  let presses = 0, clicks = 0, scrolls = 0, movementPx = 0;
  for (const event of events) {
    const data = event.data ?? {};
    if (isFiniteNumber(data.presses)) presses += data.presses;
    if (isFiniteNumber(data.clicks)) clicks += data.clicks;
    if (isFiniteNumber(data.scrolls)) scrolls += data.scrolls;
    const dx = isFiniteNumber(data.deltaX) ? Math.abs(data.deltaX) : 0;
    const dy = isFiniteNumber(data.deltaY) ? Math.abs(data.deltaY) : 0;
    movementPx += dx + dy;
  }
  return { presses, clicks, scrolls, movementPx: Math.round(movementPx) };
}

// ── Any other bucket type — pass its raw data through generically ────────

export interface GenericBucketSummary {
  bucketId: string;
  bucketType: string;
  eventCount: number;
  /** JSON-stringified `data` payloads, capped in count and length. Rendered
   * as plain text only — never HTML — by the caller. */
  samples: string[];
}

const MAX_GENERIC_SAMPLES = 10;
const MAX_SAMPLE_CHARS = 300;

export function summarizeGenericBucket(bucketId: string, bucketType: string, events: AwEvent[]): GenericBucketSummary {
  const samples = events.slice(0, MAX_GENERIC_SAMPLES).map((event) => {
    const json = JSON.stringify(event.data ?? {});
    return json.length > MAX_SAMPLE_CHARS ? `${json.slice(0, MAX_SAMPLE_CHARS)}…` : json;
  });
  return { bucketId, bucketType, eventCount: events.length, samples };
}

// ── Bucket-type routing ───────────────────────────────────────────────────

export type KnownBucketKind = "window" | "afk" | "web" | "vscode" | "input" | "generic";

export function classifyBucketId(bucketId: string): KnownBucketKind {
  if (bucketId.startsWith("aw-watcher-window")) return "window";
  if (bucketId.startsWith("aw-watcher-afk")) return "afk";
  if (bucketId.startsWith("aw-watcher-web")) return "web";
  if (bucketId.startsWith("aw-watcher-vscode")) return "vscode";
  if (bucketId.startsWith("aw-watcher-input")) return "input";
  return "generic";
}
