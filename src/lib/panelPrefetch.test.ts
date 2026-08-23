import { describe, expect, test } from "bun:test";
import { PanelPrefetchQueue } from "./panelPrefetch";

const tick = () => new Promise<void>((resolve) => queueMicrotask(resolve));

describe("PanelPrefetchQueue", () => {
  test("caps idle imports at the configured concurrency", async () => {
    let active = 0;
    let peak = 0;
    const releases: Array<() => void> = [];
    const queue = new PanelPrefetchQueue(1);
    const load = () => new Promise<void>((resolve) => {
      active += 1;
      peak = Math.max(peak, active);
      releases.push(() => { active -= 1; resolve(); });
    });

    queue.enqueueIdle([{ id: "one", load }, { id: "two", load }]);
    await tick();
    expect(peak).toBe(1);
    expect(releases).toHaveLength(1);

    releases.shift()?.();
    await tick();
    await tick();
    expect(peak).toBe(1);
    expect(releases).toHaveLength(1);
  });

  test("drops obsolete queued prefetch after a navigation change", async () => {
    const loaded: string[] = [];
    let releaseActive: (() => void) | undefined;
    const queue = new PanelPrefetchQueue(1);
    queue.enqueueIntent("active", () => new Promise<void>((resolve) => {
      releaseActive = resolve;
    }));
    queue.enqueueIdle([
      { id: "stale", load: async () => { loaded.push("stale"); } },
      { id: "next", load: async () => { loaded.push("next"); } },
    ]);
    queue.keepRelevant("next");
    releaseActive?.();
    await tick();
    await tick();

    expect(loaded).toEqual(["next"]);
  });

  test("does not invoke an import twice while it is still in flight", async () => {
    let calls = 0;
    let release: (() => void) | undefined;
    const queue = new PanelPrefetchQueue(1);
    const load = () => new Promise<void>((resolve) => {
      calls += 1;
      release = resolve;
    });

    queue.enqueueIntent("network", load);
    queue.enqueueIntent("network", load);
    queue.enqueueIdle([{ id: "network", load }]);
    await tick();
    expect(calls).toBe(1);

    release?.();
    await tick();
    await tick();
    queue.enqueueIntent("network", load);
    expect(calls).toBe(1);
  });
});
