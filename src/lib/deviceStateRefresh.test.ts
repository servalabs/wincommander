import { describe, expect, test } from "bun:test";
import {
  announceDeviceStateRefresh,
  createDeviceStateRefresher,
  DEVICE_STATE_CHANGED_EVENT,
  onDeviceStateRefresh,
  refreshWhenIdle,
  SETTINGS_CHANGED_EVENT,
} from "./deviceStateRefresh";

function fakeClock() {
  let next = 1;
  const timers = new Map<number, () => void>();
  return {
    setTimeout: ((fn: () => void) => { const id = next++; timers.set(id, fn); return id; }) as unknown as typeof setTimeout,
    clearTimeout: ((id: number) => { timers.delete(id); }) as unknown as typeof clearTimeout,
    pending: () => timers.size,
    async fire() {
      const due = [...timers.entries()];
      timers.clear();
      for (const [, fn] of due) fn();
      await Promise.resolve();
      await Promise.resolve();
    },
  };
}

const flush = async () => { for (let i = 0; i < 5; i++) await Promise.resolve(); };

describe("native event names match what the native side emits", () => {
  test("names", () => {
    // `settings-changed` is emitted by flow_bridge::on_settings_written and
    // `device-state-changed` by the Pro agent; a rename on either side must fail here.
    expect(SETTINGS_CHANGED_EVENT).toBe("settings-changed");
    expect(DEVICE_STATE_CHANGED_EVENT).toBe("device-state-changed");
  });
});

describe("createDeviceStateRefresher", () => {
  test("a device-state notice reads settings, then announces once, after the debounce", async () => {
    const clock = fakeClock();
    const order: string[] = [];
    const refresher = createDeviceStateRefresher({
      readSettings: async () => { order.push("read"); },
      announceDevice: () => { order.push("announce"); },
      clock,
    });
    refresher.deviceStateChanged();
    expect(order).toEqual([]);
    await clock.fire();
    await flush();
    expect(order).toEqual(["read", "announce"]);
  });

  test("the burst a single command produces becomes one read and one announcement", async () => {
    const clock = fakeClock();
    let reads = 0;
    let announces = 0;
    const refresher = createDeviceStateRefresher({
      readSettings: async () => { reads++; },
      announceDevice: () => { announces++; },
      clock,
    });
    refresher.settingsChanged();
    refresher.deviceStateChanged();
    refresher.settingsChanged();
    refresher.deviceStateChanged();
    expect(clock.pending()).toBe(1);
    await clock.fire();
    await flush();
    expect(reads).toBe(1);
    expect(announces).toBe(1);
  });

  test("a settings-only change re-reads settings without waking status readers", async () => {
    const clock = fakeClock();
    let reads = 0;
    let announces = 0;
    const refresher = createDeviceStateRefresher({
      readSettings: async () => { reads++; },
      announceDevice: () => { announces++; },
      clock,
    });
    refresher.settingsChanged();
    await clock.fire();
    await flush();
    expect(reads).toBe(1);
    expect(announces).toBe(0);
  });

  test("an event during a read is not lost: one follow-up read runs afterwards", async () => {
    const clock = fakeClock();
    let release: () => void = () => undefined;
    let reads = 0;
    let announces = 0;
    const refresher = createDeviceStateRefresher({
      readSettings: () => { reads++; return new Promise<void>((resolve) => { release = resolve; }); },
      announceDevice: () => { announces++; },
      clock,
    });
    refresher.settingsChanged();
    await clock.fire();
    expect(reads).toBe(1);
    refresher.deviceStateChanged();
    expect(clock.pending()).toBe(0);
    release();
    await flush();
    expect(clock.pending()).toBe(1);
    await clock.fire();
    release();
    await flush();
    expect(reads).toBe(2);
    expect(announces).toBe(1);
  });

  test("a failed settings read is reported and status readers are still woken", async () => {
    const clock = fakeClock();
    const errors: unknown[] = [];
    let announces = 0;
    const refresher = createDeviceStateRefresher({
      readSettings: async () => { throw new Error("pipe closed"); },
      announceDevice: () => { announces++; },
      onError: (error) => errors.push(error),
      clock,
    });
    refresher.deviceStateChanged();
    await clock.fire();
    await flush();
    expect(errors).toHaveLength(1);
    expect(announces).toBe(1);
  });

  test("dispose cancels pending work and ignores later events", async () => {
    const clock = fakeClock();
    let reads = 0;
    const refresher = createDeviceStateRefresher({
      readSettings: async () => { reads++; },
      announceDevice: () => undefined,
      clock,
    });
    refresher.deviceStateChanged();
    refresher.dispose();
    refresher.deviceStateChanged();
    expect(clock.pending()).toBe(0);
    await clock.fire();
    expect(reads).toBe(0);
  });
});

describe("refreshWhenIdle", () => {
  test("retries a skipped refresh until it runs, so a change during a refresh is not lost", async () => {
    const outcomes = ["skipped", "skipped", "ran"] as const;
    let calls = 0;
    const waits: number[] = [];
    const result = await refreshWhenIdle(
      async () => outcomes[calls++],
      async (ms) => { waits.push(ms); },
    );
    expect(result).toBe("ran");
    expect(calls).toBe(3);
    expect(waits).toEqual([1000, 1000]);
  });

  test("gives up after the bounded attempts", async () => {
    let calls = 0;
    const result = await refreshWhenIdle(async () => { calls++; return "skipped"; }, async () => undefined);
    expect(result).toBe("skipped");
    expect(calls).toBe(3);
  });

  test("a refresh that runs immediately is not repeated", async () => {
    let calls = 0;
    await refreshWhenIdle(async () => { calls++; return "ran"; }, async () => undefined);
    expect(calls).toBe(1);
  });
});

describe("device-state announcements", () => {
  test("subscribers hear the announcement until they unsubscribe", () => {
    const target = new EventTarget();
    let heard = 0;
    const off = onDeviceStateRefresh(() => { heard++; }, target);
    announceDeviceStateRefresh(target);
    announceDeviceStateRefresh(target);
    off();
    announceDeviceStateRefresh(target);
    expect(heard).toBe(2);
  });
});
