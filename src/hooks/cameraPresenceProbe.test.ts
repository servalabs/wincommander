import { describe, expect, test } from "bun:test";
import { cameraAccessState, cameraDisplayState, createCameraPresenceProbe } from "./cameraPresenceProbe";

describe("dashboard camera presence", () => {
  test("separates missing hardware, unknown probes, and camera permissions", () => {
    expect(cameraDisplayState("absent", false)).toBe("absent");
    expect(cameraDisplayState("unknown", false)).toBe("allowed");
    expect(cameraDisplayState("unknown", true)).toBe("blocked");
    expect(cameraDisplayState("present", null)).toBe("unknown");
    expect(cameraDisplayState("present", true)).toBe("blocked");
    expect(cameraDisplayState("present", false)).toBe("allowed");
  });

  test("mixed and conflicting permissions remain unknown", () => {
    expect(cameraAccessState({ disabled: false, app_access_mixed: true })).toBeNull();
    expect(cameraAccessState({ disabled: false, desktop_apps_allowed: false })).toBeNull();
    expect(cameraAccessState({ disabled: true, desktop_apps_allowed: true })).toBeNull();
    expect(cameraAccessState({ disabled: false, desktop_apps_allowed: true })).toBe(false);
    expect(cameraAccessState({ disabled: true })).toBe(true);
    expect(cameraAccessState(undefined)).toBeNull();
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
    expect(calls).toBe(1);
    release("present");
    expect(await first).toBe("present");
    expect(await second).toBe("present");
    const third = probe();
    await Promise.resolve();
    expect(calls).toBe(2);
    release("invalid result");
    expect(await third).toBe("unknown");
    expect(await createCameraPresenceProbe(() => Promise.reject(new Error("denied")))()).toBe("unknown");
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
