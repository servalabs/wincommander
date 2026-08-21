import { describe, expect, test } from "bun:test";
import { FileIconQueue, MAX_CONCURRENT_FILE_ICON_REQUESTS } from "./fileIconQueue";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

describe("FileIconQueue", () => {
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
