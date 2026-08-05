// src/lib/activityWatch.ts
//
// Pure client + domain logic for reading ActivityWatch's local REST API
// (http://127.0.0.1:5600) directly — no embedded webview, no CSS hacking of
// AW's own Vue UI. This module owns bucket discovery, the window/AFK
// intersection math, and category classification.
//
// Active-time and category semantics are implemented independently here for
// the public local viewer. Rendered app/title strings remain local and are
// not reduced for classification (see `sanitizeWindowEvent`).

import type { ActivityItem, ActivityTimelineEvent } from "@/components/activity/activityData";
import { invoke } from "@tauri-apps/api/core";

const AW_BASE = "http://127.0.0.1:5600";
const HTTP_TIMEOUT_MS = 4_000;
/** A local ActivityWatch response beyond this cannot be safely held and parsed
 * in the privileged renderer. This is a hard refusal, never a truncation. */
export const MAX_AW_RESPONSE_BYTES = 64 * 1024 * 1024;
/** Limits parallel localhost I/O without omitting any selected bucket. */
export const AW_BUCKET_FETCH_CONCURRENCY = 4;
/** A legitimate window-focus event cannot span more than a device-local day. */
const MAX_EVENT_DURATION_SECS = 86_400;

export class AwUnavailableError extends Error {
  constructor(message = "ActivityWatch is not installed or not running.") {
    super(message);
    this.name = "AwUnavailableError";
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

async function readAwJson(response: Response, controller: AbortController): Promise<unknown> {
  const declaredLength = Number(response.headers.get("content-length"));
  if (Number.isFinite(declaredLength) && declaredLength > MAX_AW_RESPONSE_BYTES) {
    controller.abort();
    throw new AwUnavailableError("ActivityWatch returned an oversized response.");
  }
  if (!response.body) throw new AwUnavailableError("ActivityWatch returned an empty response.");

  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let length = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      length += value.byteLength;
      if (length > MAX_AW_RESPONSE_BYTES) {
        controller.abort();
        throw new AwUnavailableError("ActivityWatch returned an oversized response.");
      }
      chunks.push(value);
    }
  } finally {
    reader.releaseLock();
  }

  const bytes = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }
  try {
    return JSON.parse(new TextDecoder().decode(bytes)) as unknown;
  } catch {
    throw new AwUnavailableError("ActivityWatch returned invalid JSON.");
  }
}

async function fetchAwJson<T>(path: string, params?: Record<string, string>, signal?: AbortSignal): Promise<T> {
  const url = new URL(path, AW_BASE);
  if (params) for (const [k, v] of Object.entries(params)) url.searchParams.set(k, v);
  // ActivityWatch's loopback server intentionally does not emit CORS headers.
  // In a Tauri WebView that makes a healthy local API look "unreachable".
  // Ask the native process to perform the fixed, validated loopback request;
  // regular browser/test environments retain the direct-fetch fallback.
  if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
    try {
      return await invoke<T>("activity_watch_request", {
        path: `${url.pathname}${url.search}`,
      });
    } catch (error) {
      if (error instanceof AwUnavailableError) throw error;
      throw new AwUnavailableError(typeof error === "string" ? error : undefined);
    }
  }
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), HTTP_TIMEOUT_MS);
  const abort = () => controller.abort();
  signal?.addEventListener("abort", abort, { once: true });
  if (signal?.aborted) controller.abort();
  try {
    const response = await fetch(url.toString(), { signal: controller.signal });
    if (!response.ok) throw new AwUnavailableError(`ActivityWatch returned HTTP ${response.status}.`);
    return (await readAwJson(response, controller)) as T;
  } catch (error) {
    if (error instanceof AwUnavailableError) throw error;
    throw new AwUnavailableError();
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener("abort", abort);
  }
}

// ── Wire shapes ──────────────────────────────────────────────────────────

export interface AwBucketInfo {
  hostname?: string;
  type?: string;
  client?: string;
}
export type AwBucketsMap = Record<string, AwBucketInfo>;

export interface AwServerInfo {
  hostname?: string;
}

export interface AwEvent {
  timestamp: string;
  duration: number;
  data?: Record<string, unknown> | null;
}

/** ActivityWatch data comes from a separately running local service. Treat a
 * malformed event payload as an empty object so it cannot take down the
 * complete day's viewer. */
export function eventData(data: unknown): Record<string, unknown> {
  return data !== null && typeof data === "object" && !Array.isArray(data)
    ? data as Record<string, unknown>
    : {};
}

export async function fetchBuckets(signal?: AbortSignal): Promise<AwBucketsMap> {
  const payload = await fetchAwJson<unknown>("/api/0/buckets", undefined, signal);
  if (!isRecord(payload)) throw new AwUnavailableError("ActivityWatch returned invalid bucket data.");

  const buckets: AwBucketsMap = {};
  for (const [id, info] of Object.entries(payload)) {
    if (!isRecord(info)) continue;
    buckets[id] = {
      hostname: typeof info.hostname === "string" ? info.hostname : undefined,
      type: typeof info.type === "string" ? info.type : undefined,
      client: typeof info.client === "string" ? info.client : undefined,
    };
  }
  return buckets;
}

export async function fetchServerInfo(signal?: AbortSignal): Promise<AwServerInfo> {
  const payload = await fetchAwJson<unknown>("/api/0/info", undefined, signal);
  if (!isRecord(payload)) throw new AwUnavailableError("ActivityWatch returned invalid server information.");
  return { hostname: typeof payload.hostname === "string" ? payload.hostname : undefined };
}

export const fetchBucketEvents = async (
  bucketId: string,
  startIso: string,
  endIso: string,
  // ActivityWatch uses -1 for an unbounded result set. A fixed cap silently
  // drops late-day activity on busy machines, leaving the native timeline
  // incomplete while looking valid.
  limit = -1,
  signal?: AbortSignal,
): Promise<AwEvent[]> => {
  const payload = await fetchAwJson<unknown>(`/api/0/buckets/${encodeURIComponent(bucketId)}/events`, {
    start: startIso,
    end: endIso,
    limit: String(limit),
  }, signal);
  if (!Array.isArray(payload)) throw new AwUnavailableError("ActivityWatch returned invalid event data.");
  return payload.filter((event): event is AwEvent =>
    isRecord(event) && typeof event.timestamp === "string" && typeof event.duration === "number",
  ).map((event) => ({ timestamp: event.timestamp, duration: event.duration, data: event.data }));
};

export async function mapWithConcurrency<T, U>(
  items: T[],
  concurrency: number,
  operation: (item: T, index: number) => Promise<U>,
  signal?: AbortSignal,
): Promise<U[]> {
  if (!Number.isInteger(concurrency) || concurrency < 1) throw new RangeError("Concurrency must be a positive integer.");
  const results = new Array<U>(items.length);
  let nextIndex = 0;
  const worker = async () => {
    while (nextIndex < items.length) {
      if (signal?.aborted) throw new AwUnavailableError("ActivityWatch request was cancelled.");
      const index = nextIndex++;
      results[index] = await operation(items[index], index);
    }
  };
  await Promise.all(Array.from({ length: Math.min(concurrency, items.length) }, worker));
  return results;
}

// ── Local day bounds ─────────────────────────────────────────────────────

export interface DayBounds {
  startIso: string;
  endIso: string;
  startMs: number;
  endMs: number;
}

/** Device-local calendar day for `date`. If `date` is today, the upper bound
 * is "now" (there's nothing to report past the current moment); otherwise
 * it's the following local midnight. Mirrors the collector's
 * `[local midnight, now]` window for today, generalized to any past day. */
export function dayBoundsLocal(date: Date): DayBounds {
  const start = new Date(date);
  start.setHours(0, 0, 0, 0);
  const now = new Date();
  const isToday = start.toDateString() === now.toDateString();
  const end = isToday ? now : new Date(start);
  if (!isToday) end.setDate(end.getDate() + 1);
  return { startIso: start.toISOString(), endIso: end.toISOString(), startMs: start.getTime(), endMs: end.getTime() };
}

// ── Bucket selection (mirrors select_activity_buckets) ──────────────────

export interface SelectedActivityBuckets {
  windowId: string;
  afkId: string | null;
}

const WINDOW_PREFIX = "aw-watcher-window_";
const AFK_PREFIX = "aw-watcher-afk_";

/** Prefer the window bucket for THIS machine (COMPUTERNAME) — a synced AW
 * install can hold other devices' buckets, and picking one of those merely
 * because it sorted first would report a stranger's activity as the user's
 * own. Falls back to plain id order when no hostname is known yet. */
export function selectActivityBuckets(buckets: AwBucketsMap, computerName: string | null): SelectedActivityBuckets | null {
  const windowEntries = Object.entries(buckets).filter(([id]) => id.startsWith("aw-watcher-window"));
  if (windowEntries.length === 0) return null;
  windowEntries.sort(([a], [b]) => a.localeCompare(b));

  const local = computerName && computerName.trim() ? computerName : null;
  const rank = ([id, bucket]: [string, AwBucketInfo]): number => {
    if (!local) return 2;
    if (id.toLowerCase() === `${WINDOW_PREFIX}${local}`.toLowerCase()) return 0;
    if (bucket.hostname && bucket.hostname.toLowerCase() === local.toLowerCase()) return 1;
    return 2;
  };
  // Array.prototype.sort is stable — ties keep the prior id-ascending order.
  windowEntries.sort((a, b) => rank(a) - rank(b));

  // A synced ActivityWatch store can contain buckets from several machines.
  // Never substitute another machine's events when this device has no match.
  if (local && rank(windowEntries[0]) === 2) return null;

  const [windowId, windowBucket] = windowEntries[0];
  const windowHostname = windowBucket.hostname?.trim()
    ? windowBucket.hostname
    : windowId.startsWith(WINDOW_PREFIX) ? windowId.slice(WINDOW_PREFIX.length) : "";

  const afkEntries = Object.entries(buckets)
    .filter(([id]) => id.startsWith("aw-watcher-afk"))
    .sort(([a], [b]) => a.localeCompare(b));
  const exactAfkId = windowHostname ? `${AFK_PREFIX}${windowHostname}`.toLowerCase() : null;
  const exact = exactAfkId ? afkEntries.find(([id]) => id.toLowerCase() === exactAfkId) : undefined;
  const hostMatch = !exact && windowHostname
    ? afkEntries.find(([, bucket]) => bucket.hostname?.toLowerCase() === windowHostname.toLowerCase())
    : undefined;

  return { windowId, afkId: (exact ?? hostMatch)?.[0] ?? null };
}

// ── AFK active intervals + timeline intersection ─────────────────────────

export interface UtcInterval {
  startMs: number;
  endMs: number;
}

/** Validate + clip one event's `[start, start+duration]` to `[lowerMs,
 * upperMs]`. Shared by AFK-status parsing and any other bucket type that
 * needs the same "is this a sane, in-range interval" check. */
export function clipEventInterval(event: AwEvent, lowerMs: number, upperMs: number): UtcInterval | null {
  if (!Number.isFinite(event.duration) || event.duration <= 0 || event.duration > MAX_EVENT_DURATION_SECS) return null;
  const startMs = Date.parse(event.timestamp);
  if (!Number.isFinite(startMs)) return null;
  const endMs = startMs + Math.round(event.duration * 1000);
  const clippedStart = Math.max(startMs, lowerMs);
  const clippedEnd = Math.min(endMs, upperMs);
  return clippedEnd > clippedStart ? { startMs: clippedStart, endMs: clippedEnd } : null;
}

/** Mirrors `active_intervals`. `null` = the AFK bucket had no usable status
 * events — caller may fall back to raw window activity. `[]` = usable AFK
 * data explicitly says the user was never active in this window, which must
 * NOT fall back to raw window events (that would misreport idle time as
 * active time). */
export function activeIntervals(events: AwEvent[], lowerMs: number, upperMs: number): UtcInterval[] | null {
  let sawUsableState = false;
  const intervals: UtcInterval[] = [];
  for (const event of events) {
    const status = eventData(event.data).status;
    if (status !== "afk" && status !== "not-afk") continue;
    const interval = clipEventInterval(event, lowerMs, upperMs);
    if (!interval) continue;
    sawUsableState = true;
    if (status === "not-afk") intervals.push(interval);
  }
  if (!sawUsableState) return null;

  intervals.sort((a, b) => a.startMs - b.startMs);
  const merged: UtcInterval[] = [];
  for (const interval of intervals) {
    const previous = merged[merged.length - 1];
    if (previous && interval.startMs <= previous.endMs) {
      previous.endMs = Math.max(previous.endMs, interval.endMs);
      continue;
    }
    merged.push({ ...interval });
  }
  return merged;
}

export interface RawTimelineEvent {
  timestampMs: number;
  /** Seconds. */
  duration: number;
  app: string;
  title: string;
  categoryPath: string[];
  categoryColor?: string;
}

/** Mirrors `intersect_timeline`: a window event crossing two active periods
 * becomes two exact records, so downstream totals/charts exclude the same
 * idle portions the AFK watcher observed. */
export function intersectTimeline(events: RawTimelineEvent[], active: UtcInterval[]): RawTimelineEvent[] {
  const sorted = [...events].sort((a, b) => a.timestampMs - b.timestampMs);
  const result: RawTimelineEvent[] = [];
  let activeCursor = 0;
  for (const event of sorted) {
    const start = event.timestampMs;
    const end = start + Math.round(event.duration * 1000);
    while (activeCursor < active.length && active[activeCursor].endMs <= start) activeCursor++;
    let index = activeCursor;
    while (index < active.length && active[index].startMs < end) {
      const overlapStart = Math.max(start, active[index].startMs);
      const overlapEnd = Math.min(end, active[index].endMs);
      if (overlapEnd > overlapStart) {
        result.push({ ...event, timestampMs: overlapStart, duration: (overlapEnd - overlapStart) / 1000 });
      }
      index++;
    }
  }
  return result;
}

// ── Category classification (mirrors compile_categories / classify_event) ─

export interface AwCategory {
  name: string[];
  rule?: { type?: string; regex?: string; ignore_case?: boolean };
  data?: { color?: string };
}
export interface AwSettings {
  classes?: AwCategory[];
}

export interface CompiledCategory {
  path: string[];
  regex: RegExp;
  color?: string;
}

/** Keep classification aligned with the fleet collector's explicit
 * code-point bound. This constrains only the regex input; the viewer keeps
 * the original app/title strings intact for local display. */
export const CLASSIFY_INPUT_LIMIT = 512;
const MAX_CATEGORY_REGEX_CHARS = 512;
const MAX_CATEGORY_REGEX_QUANTIFIERS = 1;

function classificationInput(value: string): string {
  return [...value].slice(0, CLASSIFY_INPUT_LIMIT).join("");
}

function sameName(a: string[], b: string[]): boolean {
  return a.length === b.length && a.every((v, i) => v === b[i]);
}

/** JavaScript regexes run on the renderer's main thread. Categories are a
 * display aid, so reject patterns outside this small linear-time subset
 * instead of allowing a local settings payload to freeze the application. */
export function isSafeCategoryRegex(source: string): boolean {
  if (!source || source.length > MAX_CATEGORY_REGEX_CHARS) return false;
  if (/\\[1-9]|\\k<|\(\?(?:=|!|<=|<!)/.test(source)) return false;

  let inCharacterClass = false;
  let escaped = false;
  let quantifiers = 0;
  for (let index = 0; index < source.length; index++) {
    const char = source[index];
    if (escaped) {
      escaped = false;
      continue;
    }
    if (char === "\\") {
      escaped = true;
      continue;
    }
    if (char === "[") {
      inCharacterClass = true;
      continue;
    }
    if (char === "]") {
      inCharacterClass = false;
      continue;
    }
    if (inCharacterClass) continue;

    const isNonCapturingGroupPrefix = char === "?" && source[index - 1] === "(" && source[index + 1] === ":";
    const isBraceQuantifier = char === "{" && /^\{\d+(?:,\d*)?\}/.test(source.slice(index));
    if (char === ")" && /^(?:[+*?]|\{\d+(?:,\d*)?\})/.test(source.slice(index + 1))) return false;
    if (!isNonCapturingGroupPrefix && (char === "*" || char === "+" || char === "?" || isBraceQuantifier)) {
      quantifiers++;
      if (quantifiers > MAX_CATEGORY_REGEX_QUANTIFIERS) return false;
    }
  }
  return !escaped && !inCharacterClass;
}

/** A category with no colour of its own inherits its nearest ANCESTOR's
 * colour (not sibling/descendant) — walk up `name` one segment at a time. */
export function inheritedCategoryColor(category: AwCategory, all: AwCategory[]): string | undefined {
  if (category.data?.color) return category.data.color;
  let parent = category.name;
  while (parent.length > 1) {
    parent = parent.slice(0, -1);
    const match = all.find((c) => sameName(c.name, parent));
    if (match?.data?.color) return match.data.color;
  }
  return undefined;
}

export function compileCategories(classes: AwCategory[]): CompiledCategory[] {
  const compiled: CompiledCategory[] = [];
  for (const category of classes) {
    if (category.rule?.type !== "regex" || !category.name?.length || !category.rule.regex) continue;
    if (!isSafeCategoryRegex(category.rule.regex)) continue;
    let regex: RegExp;
    try {
      // "m" (multiline ^/$) matches Rust's `RegexBuilder::multi_line(true)`.
      regex = new RegExp(category.rule.regex, category.rule.ignore_case ? "im" : "m");
    } catch {
      continue;
    }
    compiled.push({ path: category.name, regex, color: inheritedCategoryColor(category, classes) });
  }
  return compiled;
}

/** The DEEPEST matching category wins; on a tie, the LAST one encountered
 * wins (matches Rust `Iterator::max_by_key`, which returns the last max). */
export function classifyEvent(app: string, title: string, categories: CompiledCategory[]): { path: string[]; color?: string } {
  const classificationApp = classificationInput(app);
  const classificationTitle = classificationInput(title);
  let best: CompiledCategory | null = null;
  for (const category of categories) {
    if (category.regex.test(classificationApp) || category.regex.test(classificationTitle)) {
      if (!best || category.path.length >= best.path.length) best = category;
    }
  }
  return best ? { path: best.path, color: best.color } : { path: ["Uncategorized"], color: "#CCC" };
}

export async function loadCategories(signal?: AbortSignal): Promise<CompiledCategory[]> {
  try {
    const settings = await fetchAwJson<unknown>("/api/0/settings", undefined, signal);
    return isRecord(settings) && Array.isArray(settings.classes)
      ? compileCategories(settings.classes as AwCategory[])
      : [];
  } catch {
    // Categories are a display nicety — never block the timeline on them.
    return [];
  }
}

// ── Window-event sanitization ─────────────────────────────────────────────

// Defensive cap against a corrupt/pathological AW event, NOT a fidelity
// limit — the owner wants full window titles/URLs, so this is generous.
const MAX_FIELD_CHARS = 4000;

function truncate(value: string, max: number): string {
  return value.length > max ? value.slice(0, max) : value;
}

/** Validate, clip to `[dayStartMs, nowMs]`, and classify one raw AW window
 * event. Returns null for anything malformed or fully clipped away. */
export function sanitizeWindowEvent(
  event: AwEvent,
  dayStartMs: number,
  nowMs: number,
  categories: CompiledCategory[],
): RawTimelineEvent | null {
  if (!Number.isFinite(event.duration) || event.duration <= 0 || event.duration > MAX_EVENT_DURATION_SECS) return null;
  const startMs = Date.parse(event.timestamp);
  if (!Number.isFinite(startMs)) return null;
  const endMs = startMs + Math.round(event.duration * 1000);
  const clippedStart = Math.max(startMs, dayStartMs);
  const clippedEnd = Math.min(endMs, nowMs);
  const durationSecs = (clippedEnd - clippedStart) / 1000;
  if (durationSecs <= 0) return null;

  const data = eventData(event.data);
  const app = truncate(typeof data.app === "string" ? data.app : "", MAX_FIELD_CHARS);
  const title = truncate(typeof data.title === "string" ? data.title : "", MAX_FIELD_CHARS);
  if (!app && !title) return null;

  const { path, color } = classifyEvent(app, title, categories);
  return { timestampMs: clippedStart, duration: durationSecs, app, title, categoryPath: path, categoryColor: color };
}

// ── Aggregation into the shared viewer components' shapes ────────────────

function localClockSeconds(timestampMs: number): number {
  const timestamp = new Date(timestampMs);
  return timestamp.getHours() * 3600 + timestamp.getMinutes() * 60 + timestamp.getSeconds();
}

/** Timeline coordinates are local wall-clock time, not elapsed milliseconds
 * from midnight. The two differ after a DST fallback; elapsed time would
 * push the final local hour past this viewer's 24-hour axis and erase it. */
export function toActivityTimelineEvents(events: RawTimelineEvent[]): ActivityTimelineEvent[] {
  return events.map((event, index) => {
    const startSeconds = localClockSeconds(event.timestampMs);
    const endSeconds = Math.max(startSeconds, startSeconds + Math.round(event.duration));
    return {
      id: `${event.timestampMs}-${index}`,
      startSeconds,
      endSeconds,
      label: event.title || event.app || "Unknown",
      app: event.app,
      title: event.title,
      categoryPath: event.categoryPath,
      color: event.categoryColor,
    };
  });
}

export function summarizeByField(events: RawTimelineEvent[], field: "app" | "title"): ActivityItem[] {
  const totals = new Map<string, number>();
  for (const event of events) {
    const key = event[field];
    if (!key) continue;
    totals.set(key, (totals.get(key) ?? 0) + event.duration);
  }
  return [...totals.entries()]
    .map(([label, seconds]) => ({ label, seconds }))
    .sort((a, b) => b.seconds - a.seconds);
}

export { AW_BASE };
