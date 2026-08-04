import { describe, expect, it } from "bun:test";
import {
  AW_BUCKET_FETCH_CONCURRENCY,
  AwUnavailableError,
  CLASSIFY_INPUT_LIMIT,
  MAX_AW_RESPONSE_BYTES,
  classifyEvent,
  compileCategories,
  dayBoundsLocal,
  fetchBucketEvents,
  fetchBuckets,
  isSafeCategoryRegex,
  mapWithConcurrency,
  sanitizeWindowEvent,
  selectActivityBuckets,
  toActivityTimelineEvents,
} from "./activityWatch";

declare const process: { env: Record<string, string | undefined> };

describe("ActivityWatch event retrieval", () => {
  it("requests the complete selected-day event range by default", async () => {
    const originalFetch = globalThis.fetch;
    let requestedUrl = "";
    globalThis.fetch = async (input) => {
      requestedUrl = String(input);
      return new Response("[]", { status: 200, headers: { "content-type": "application/json" } });
    };

    try {
      await fetchBucketEvents("aw-watcher-window_test host", "2026-08-04T00:00:00.000Z", "2026-08-05T00:00:00.000Z");
    } finally {
      globalThis.fetch = originalFetch;
    }

    const url = new URL(requestedUrl);
    expect(url.origin).toBe("http://127.0.0.1:5600");
    expect(url.searchParams.get("limit")).toBe("-1");
  });

  it("aborts an in-flight local request when its caller changes view", async () => {
    const originalFetch = globalThis.fetch;
    let receivedSignal: AbortSignal | undefined;
    globalThis.fetch = (_input, init) => new Promise<Response>((_resolve, reject) => {
      receivedSignal = init?.signal ?? undefined;
      receivedSignal?.addEventListener("abort", () => reject(new Error("aborted")), { once: true });
    });

    const controller = new AbortController();
    const pending = fetchBuckets(controller.signal);
    controller.abort();
    let failed = false;
    try {
      await pending;
    } catch (error) {
      failed = error instanceof AwUnavailableError;
    } finally {
      globalThis.fetch = originalFetch;
    }

    expect(receivedSignal?.aborted).toBe(true);
    expect(failed).toBe(true);
  });

  it("rejects an oversized response instead of buffering an unbounded payload", async () => {
    const originalFetch = globalThis.fetch;
    globalThis.fetch = async () => new Response("[]", {
      status: 200,
      headers: { "content-length": String(MAX_AW_RESPONSE_BYTES + 1) },
    });

    let failed = false;
    try {
      await fetchBuckets();
    } catch (error) {
      failed = error instanceof AwUnavailableError;
    } finally {
      globalThis.fetch = originalFetch;
    }

    expect(failed).toBe(true);
  });

  it("bounds optional bucket requests while retaining result order", async () => {
    let active = 0;
    let maximumActive = 0;
    const result = await mapWithConcurrency(Array.from({ length: 10 }, (_, index) => index), AW_BUCKET_FETCH_CONCURRENCY, async (value) => {
      active++;
      maximumActive = Math.max(maximumActive, active);
      await new Promise<void>((resolve) => setTimeout(resolve, 1));
      active--;
      return value * 2;
    });

    expect(maximumActive).toBe(AW_BUCKET_FETCH_CONCURRENCY);
    expect(result).toEqual([0, 2, 4, 6, 8, 10, 12, 14, 16, 18]);
  });

  it("does not dequeue later bucket requests after cancellation", async () => {
    const controller = new AbortController();
    let started = 0;
    const pending = mapWithConcurrency(Array.from({ length: 10 }, (_, index) => index), AW_BUCKET_FETCH_CONCURRENCY, async () => {
      started++;
      await new Promise<void>((resolve) => controller.signal.addEventListener("abort", () => resolve(), { once: true }));
      return 0;
    }, controller.signal);
    controller.abort();

    let failed = false;
    try {
      await pending;
    } catch (error) {
      failed = error instanceof AwUnavailableError;
    }

    expect(started).toBe(AW_BUCKET_FETCH_CONCURRENCY);
    expect(failed).toBe(true);
  });
});

describe("ActivityWatch category classification", () => {
  it("uses the fleet-compatible 512-code-point bound without truncating rendered text", () => {
    const categories = compileCategories([
      { name: ["Bounded"], rule: { type: "regex", regex: "x$" } },
    ]);
    const app = `${"😀".repeat(CLASSIFY_INPUT_LIMIT - 1)}x-after-the-bound`;

    expect(classifyEvent(app, "", categories).path).toEqual(["Bounded"]);
  });

  it("rejects patterns that can backtrack catastrophically on the renderer thread", () => {
    expect(isSafeCategoryRegex("(a+)+$")).toBe(false);
    expect(isSafeCategoryRegex("^(a|aa)+$")).toBe(false);
    expect(isSafeCategoryRegex(".*project.*")).toBe(false);
    expect(isSafeCategoryRegex("^(?=admin).*")).toBe(false);
    expect(isSafeCategoryRegex("^Code\\.exe$")).toBe(true);

    expect(compileCategories([
      { name: ["Unsafe"], rule: { type: "regex", regex: "(a+)+$" } },
      { name: ["Safe"], rule: { type: "regex", regex: "^Code\\.exe$" } },
    ])).toHaveLength(1);
  });
});

describe("ActivityWatch local-device selection", () => {
  it("refuses a remote-only window bucket instead of displaying another machine's activity", () => {
    expect(selectActivityBuckets({
      "aw-watcher-window_remote": { hostname: "remote" },
    }, "local")).toBeNull();
  });

  it("prefers the matching local window bucket in a synced store", () => {
    expect(selectActivityBuckets({
      "aw-watcher-window_remote": { hostname: "remote" },
      "aw-watcher-window_local": { hostname: "local" },
    }, "local")).toMatchObject({ windowId: "aw-watcher-window_local" });
  });
});

describe("ActivityWatch local-day bounds", () => {
  it("keeps the complete DST fallback day through the next local midnight", () => {
    const previousTimezone = process.env.TZ;
    process.env.TZ = "America/New_York";
    try {
      const bounds = dayBoundsLocal(new Date(2026, 10, 1));
      expect(bounds.endMs - bounds.startMs).toBe(25 * 60 * 60 * 1000);

      const lateEvent = toActivityTimelineEvents([{
        timestampMs: new Date(2026, 10, 1, 23, 30).getTime(),
        duration: 60,
        app: "Editor",
        title: "late-day.txt",
        categoryPath: ["Uncategorized"],
      }]);
      expect(lateEvent[0].startSeconds).toBe(23 * 3600 + 30 * 60);
    } finally {
      process.env.TZ = previousTimezone;
    }
  });
});

describe("ActivityWatch malformed records", () => {
  it("skips a null data payload instead of throwing away the entire day", () => {
    expect(sanitizeWindowEvent(
      { timestamp: "2026-08-04T10:00:00.000Z", duration: 60, data: null },
      Date.parse("2026-08-04T00:00:00.000Z"),
      Date.parse("2026-08-05T00:00:00.000Z"),
      [],
    )).toBeNull();
  });
});
