import { describe, expect, test } from "bun:test";
import { FileIconQueue, MAX_CONCURRENT_FILE_ICON_REQUESTS } from "./fileIconQueue";
import { MAX_CACHED_FILE_ICONS } from "./fileIconCache";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

describe("FileIconQueue", () => {
  test("discards expired failed lookups before evicting a still-valid icon", () => {
    let now = 0;
    const queue = new FileIconQueue(async () => null, 1, { maxEntries: 2, ttlMs: 100, negativeTtlMs: 10, now: () => now });
    queue.prime("live", "data");
    queue.prime("dead", null);
    now = 11;
    queue.prime("fresh", "data");
    expect(queue.get("live")).toBe("data");
    expect(queue.get("dead")).toBeUndefined();
    expect(queue.get("fresh")).toBe("data");
  });

  test("a long browse retains only the configured recent icon population", () => {
    const queue = new FileIconQueue(async () => null);
    const paths = Array.from({ length: 10_000 }, (_, index) => `file-${index}`);
    for (const path of paths) queue.prime(path, `icon-${path}`);
    const retained = paths.filter((path) => queue.get(path) !== undefined);
    expect(retained).toHaveLength(MAX_CACHED_FILE_ICONS);
    expect(retained).toEqual(paths.slice(-MAX_CACHED_FILE_ICONS));
  });

  test("replacing an entry releases its prior byte cost", () => {
    const queue = new FileIconQueue(async () => null, 1, { maxBytes: 12 });
    queue.prime("a", "12345");
    queue.prime("a", "1");
    queue.prime("b", "123");
    expect(queue.get("a")).toBe("1");
    expect(queue.get("b")).toBe("123");
  });

  test("zero cache capacity still delivers native results", async () => {
    const received: (string | null)[] = [];
    const queue = new FileIconQueue(async () => "icon", 1, { maxEntries: 0 });
    queue.request("file", 0, (data) => received.push(data));
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(received).toEqual(["icon"]);
    expect(queue.get("file")).toBeUndefined();
  });

  test("evicts the least recently used icon when the entry budget is full", () => {
    const queue = new FileIconQueue(async () => null, 1, { maxEntries: 2 });
    queue.prime("a", "icon-a");
    queue.prime("b", "icon-b");
    expect(queue.get("a")).toBe("icon-a");
    queue.prime("c", "icon-c");
    expect(queue.get("b")).toBeUndefined();
    expect(queue.get("a")).toBe("icon-a");
    expect(queue.get("c")).toBe("icon-c");
  });

  test("bounds retained string bytes without retaining an oversized icon", () => {
    const queue = new FileIconQueue(async () => null, 1, { maxBytes: 20 });
    queue.prime("one", "aaaaa"); // 16 bytes including the UTF-16 path.
    queue.prime("two", "bb"); // 10 bytes; the old entry must leave.
    expect(queue.get("one")).toBeUndefined();
    queue.prime("huge", "x".repeat(20));
    expect(queue.get("huge")).toBeUndefined();
    expect(queue.get("two")).toBe("bb");
  });

  test("expires successful and negative lookups without a recurring timer", () => {
    let now = 0;
    const queue = new FileIconQueue(async () => null, 1, { ttlMs: 100, negativeTtlMs: 10, now: () => now });
    queue.prime("icon", "data");
    queue.prime("missing", null);
    now = 9;
    expect(queue.get("missing")).toBeNull();
    now = 10;
    expect(queue.get("missing")).toBeUndefined();
    expect(queue.get("icon")).toBe("data");
    now = 100;
    expect(queue.get("icon")).toBeUndefined();
  });

  test("a transient loader failure retries after the negative-cache lifetime", async () => {
    let now = 0;
    let loads = 0;
    const received: (string | null)[] = [];
    const queue = new FileIconQueue(async () => {
      loads += 1;
      if (loads === 1) throw new Error("shell temporarily unavailable");
      return "recovered";
    }, 1, { negativeTtlMs: 10, now: () => now });
    queue.request("file", 0, (data) => received.push(data));
    await new Promise((resolve) => setTimeout(resolve, 0));
    queue.request("file", 0, (data) => received.push(data));
    expect(loads).toBe(1);
    now = 10;
    queue.request("file", 0, (data) => received.push(data));
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(loads).toBe(2);
    expect(received).toEqual([null, null, "recovered"]);
  });

  test("cache eviction preserves in-flight sharing and active subscribers", async () => {
    let loads = 0;
    const loading = deferred<string | null>();
    const received: (string | null)[] = [];
    const queue = new FileIconQueue(() => { loads += 1; return loading.promise; }, 1, { maxEntries: 1 });
    queue.request("loading", 0, (data) => received.push(data));
    await Promise.resolve();
    queue.prime("old", "old-icon");
    queue.prime("new", "new-icon");
    queue.request("loading", 0, (data) => received.push(data));
    loading.resolve("loaded-icon");
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(loads).toBe(1);
    expect(received).toEqual(["loaded-icon", "loaded-icon"]);
    expect(queue.get("new")).toBeUndefined();
    expect(queue.get("loading")).toBe("loaded-icon");
  });

  test("an oversized result still reaches the row without occupying cache", async () => {
    const received: (string | null)[] = [];
    const queue = new FileIconQueue(async () => "x".repeat(20), 1, { maxBytes: 10 });
    queue.request("file", 0, (data) => received.push(data));
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(received).toEqual(["x".repeat(20)]);
    expect(queue.get("file")).toBeUndefined();
  });

  test("caps native icon IPC at eight concurrent requests", () => {
    expect(MAX_CONCURRENT_FILE_ICON_REQUESTS).toBe(8);
  });

  test("starts visible requests first and never exceeds its concurrency budget", async () => {
    const started: string[] = [];
    const loaders = new Map<string, ReturnType<typeof deferred<string | null>>>();
    const queue = new FileIconQueue((path) => {
      started.push(path);
      const next = deferred<string | null>();
      loaders.set(path, next);
      return next.promise;
    }, 2);

    queue.request("far", 9, () => {});
    queue.request("visible", 0, () => {});
    queue.request("near", 1, () => {});
    await Promise.resolve();

    expect(started).toEqual(["visible", "near"]);
    loaders.get("visible")?.resolve("visible-icon");
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(started).toEqual(["visible", "near", "far"]);
  });

  test("drops a queued icon when its stale result row unmounts", async () => {
    const started: string[] = [];
    const first = deferred<string | null>();
    const queue = new FileIconQueue((path) => {
      started.push(path);
      return first.promise;
    }, 1);

    queue.request("current", 0, () => {});
    const cancelStale = queue.request("stale", 1, () => {});
    cancelStale();
    await Promise.resolve();

    expect(started).toEqual(["current"]);
    first.resolve(null);
    await Promise.resolve();
    await Promise.resolve();
    expect(started).toEqual(["current"]);
  });
});
