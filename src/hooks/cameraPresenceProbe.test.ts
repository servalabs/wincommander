import { describe, expect, test } from "bun:test";
import { cameraAccessState, cameraDisplayState, createCameraPresenceProbe, createCameraPresenceStore, hasNoDetectedCamera } from "./cameraPresenceProbe";

describe("dashboard camera presence", () => {
  test("separates missing hardware, unknown probes, and camera permissions", () => {
    expect(cameraDisplayState("absent", false)).toBe("absent");
    expect(cameraDisplayState("absent", true)).toBe("absent");
    expect(cameraDisplayState("absent", null)).toBe("absent");
    expect(cameraDisplayState("unknown", false)).toBe("unknown");
    expect(cameraDisplayState("unknown", true)).toBe("unknown");
    expect(cameraDisplayState("present", null)).toBe("unknown");
    expect(cameraDisplayState("present", true)).toBe("blocked");
    expect(cameraDisplayState("present", false)).toBe("allowed");
  });

  test("mixed permissions remain actionable while failed reads remain unknown", () => {
    expect(cameraAccessState({ disabled: false, app_access_mixed: true })).toBe(false);
    expect(cameraAccessState({ disabled: false, desktop_apps_allowed: false })).toBe(false);
    expect(cameraAccessState({ disabled: true, desktop_apps_allowed: true })).toBe(true);
    expect(cameraAccessState({ disabled: false, desktop_apps_allowed: true })).toBe(false);
    expect(cameraAccessState({ disabled: true })).toBe(true);
    expect(cameraAccessState(undefined)).toBeNull();
  });

  test("host absence survives sharing and stays separate from confirmed session absence", async () => {
    const store = createCameraPresenceStore();
    const read = createCameraPresenceProbe(async () => "host_absent");
    store.publish(await read(), store.epoch());
    expect(store.get()).toBe("host_absent");
    expect(hasNoDetectedCamera(store.get())).toBe(true);
    expect(cameraDisplayState("host_absent", false)).toBe("absent");
    expect(cameraDisplayState("host_absent", true)).toBe("absent");
    store.reset();
    expect(hasNoDetectedCamera(store.get())).toBe(false);
  });

  test("shares an in-flight query and treats failure as unknown", async () => {
    let calls = 0;
    let release!: (value: unknown) => void;
    const probe = createCameraPresenceProbe(() => {
      calls++;
      return new Promise(resolve => { release = resolve; });
    });
    const first = probe();
    const second = probe();
    await Promise.resolve();
    await Promise.resolve();
    expect(calls).toBe(1);
    release("present");
    expect(await first).toBe("present");
    expect(await second).toBe("present");
    const third = probe();
    await Promise.resolve();
    await Promise.resolve();
    expect(calls).toBe(2);
    release("invalid result");
    expect(await third).toBe("unknown");
    expect(await createCameraPresenceProbe(() => Promise.reject(new Error("denied")))()).toBe("unknown");
  });

  test("shared hardware evidence survives remount and rejects reads from before recheck", () => {
    const store = createCameraPresenceStore();
    const observed: (string | null)[] = [];
    const unmount = store.subscribe(value => observed.push(value));
    const oldRead = store.epoch();
    store.publish("absent", oldRead);
    unmount();
    expect(store.get()).toBe("absent");
    const remount = store.subscribe(value => observed.push(value));
    store.reset();
    store.publish("absent", oldRead);
    expect(store.get()).toBeNull();
    store.publish("present", store.epoch());
    store.publish("unknown", store.epoch());
    expect(store.get()).toBe("present");
    expect(observed).toEqual(["absent", null, "present"]);
    remount();
  });

  test("a remount after recheck runs a fresh read after the old read completes", async () => {
    const store = createCameraPresenceStore();
    let calls = 0;
    let release!: (value: unknown) => void;
    const probe = createCameraPresenceProbe(() => {
      calls++;
      return calls === 1 ? new Promise(resolve => { release = resolve; }) : Promise.resolve("present");
    });
    const oldGeneration = store.epoch();
    const oldRead = probe(oldGeneration);
    await Promise.resolve();
    await Promise.resolve();
    expect(calls).toBe(1);
    store.reset();
    const newGeneration = store.epoch();
    const newRead = probe(newGeneration);
    release("absent");
    store.publish(await oldRead, oldGeneration);
    expect(store.get()).toBeNull();
    store.publish(await newRead, newGeneration);
    expect(calls).toBe(2);
    expect(store.get()).toBe("present");
  });

  test("a stalled IPC query expires and later queries can proceed", async () => {
    let calls = 0;
    const probe = createCameraPresenceProbe(() => {
      calls++;
      return calls === 1 ? new Promise(() => {}) : Promise.resolve("present");
    }, 10);
    expect(await probe()).toBe("unknown");
    expect(await probe()).toBe("present");
    expect(calls).toBe(2);
  });
});
