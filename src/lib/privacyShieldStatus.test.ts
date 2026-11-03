import { describe, expect, test } from "bun:test";
import { ShieldStatusGate, hasVerifiedRunningDetector, shouldProbeShieldCamera, verifiedShieldStatus } from "./privacyShieldStatus";

describe("Privacy Shield verified status", () => {
  test("a failed Start reconciles only a fresh ready detector", () => {
    expect(hasVerifiedRunningDetector({ success: true, data: { running: true, detectorProcessRunning: true, shieldLifecycle: "running" } })).toBe(true);
    for (const value of [
      { success: false, data: { running: true, detectorProcessRunning: true, shieldLifecycle: "running" } },
      { success: true, data: { running: true, detectorProcessRunning: true, shieldLifecycle: "starting" } },
      { success: true, data: { running: true, detectorProcessRunning: false, shieldLifecycle: "running" } },
      { success: true, data: { running: true, detectorProcessRunning: true, shieldLifecycle: "running", error: true } },
      { success: true, data: {} },
    ]) expect(hasVerifiedRunningDetector(value)).toBe(false);
  });
  test("accepts only a confirmed running or stopped lifecycle", () => {
    for (const value of [null, {}, { running: true, phase: "starting" },
      { running: false, phase: "running" }, { running: true, phase: "stopped" }]) {
      expect(verifiedShieldStatus(value)).toBeNull();
    }
    expect(verifiedShieldStatus({ running: true, phase: "running", activeMode: "notify_only" }))
      .toEqual({ running: true, phase: "running", activeMode: "notify_only" });
    expect(verifiedShieldStatus({ running: false, phase: "stopped", activeMode: "blur_notify" })?.activeMode)
      .toBeUndefined();
  });

  test("a readiness event updates during Start and invalidates the older stopped poll", async () => {
    const gate = new ShieldStatusGate();
    const observations: boolean[] = [];
    let finish!: (running: boolean) => void;
    const read = new Promise<boolean>(resolve => { finish = resolve; });
    const revision = gate.beginRead()!;
    const pending = read.then(running => {
      if (gate.canApply(revision)) observations.push(running);
      return gate.finishRead(revision);
    });
    const action = gate.beginAction()!;
    expect(action).not.toBeNull();
    const event = gate.acceptEvent({ running: true, phase: "running" });
    observations.push(event!.running);
    expect(gate.actionPending).toBe(true);
    finish(false);
    expect(await pending).toBe(false);
    expect(observations).toEqual([true]);
    gate.endAction(action);
    const fresh = gate.beginRead()!;
    expect(gate.canApply(fresh)).toBe(true);
    expect(gate.finishRead(fresh)).toBe(false);
  });

  test("a stopped event cannot be overwritten by an older running poll", () => {
    const gate = new ShieldStatusGate();
    const oldRead = gate.beginRead()!;
    expect(gate.acceptEvent({ running: false, phase: "stopped" })?.running).toBe(false);
    expect(gate.canApply(oldRead)).toBe(false);
    expect(gate.beginRead()).toBeNull();
    expect(gate.finishRead(oldRead)).toBe(true);
    expect(gate.canApply(gate.beginRead()!)).toBe(true);
  });

  test("polls do not overlap and unmounted cards cannot apply a late read", () => {
    const gate = new ShieldStatusGate();
    const first = gate.beginRead()!;
    expect(gate.beginRead()).toBeNull();
    expect(gate.finishRead(first)).toBe(false);
    const second = gate.beginRead()!;
    gate.setActive(false);
    expect(gate.canApply(second)).toBe(false);
    expect(gate.acceptEvent({ running: true, phase: "running" })).toBeNull();
    expect(gate.finishRead(second)).toBe(false);
    gate.setActive(true);
    expect(gate.beginRead()).not.toBeNull();
  });

  test("a late Start result cannot overwrite a newer stopped event or update an unmounted card", () => {
    const gate = new ShieldStatusGate();
    const action = gate.beginAction()!;
    expect(gate.canApplyAction(action)).toBe(true);
    gate.acceptEvent({ running: false, phase: "stopped" });
    expect(gate.canApplyAction(action)).toBe(false);
    expect(gate.endAction(action)).toBe(true);
    const next = gate.beginAction()!;
    expect(gate.endAction(action)).toBe(false);
    expect(gate.actionPending).toBe(true);
    gate.setActive(false);
    expect(gate.canApplyAction(next)).toBe(false);
    expect(gate.endAction(next)).toBe(false);
  });

  test("a long action allows status recovery without admitting duplicate actions", () => {
    const gate = new ShieldStatusGate();
    const action = gate.beginAction()!;
    expect(gate.beginRead()).toBeNull();
    gate.allowActionStatus(action);
    const read = gate.beginRead()!;
    expect(gate.canApply(read)).toBe(true);
    expect(gate.beginAction()).toBeNull();
    expect(gate.finishRead(read)).toBe(false);
    gate.endAction(action);
  });

  test("a manual refresh is not lost behind an in-flight lightweight status read", () => {
    const gate = new ShieldStatusGate();
    const ongoing = gate.beginRead()!;
    gate.queueRead();
    expect(gate.beginRead()).toBeNull();
    expect(gate.finishRead(ongoing)).toBe(true);
    expect(gate.beginRead()).not.toBeNull();
    expect(shouldProbeShieldCamera(10_000, 12_000, true)).toBe(true);
  });

  test("process status polls can run every three seconds while hardware enumeration stays sparse", () => {
    expect(shouldProbeShieldCamera(0, 100_000, false)).toBe(true);
    expect(shouldProbeShieldCamera(100_000, 103_000, false)).toBe(false);
    expect(shouldProbeShieldCamera(100_000, 129_999, false)).toBe(false);
    expect(shouldProbeShieldCamera(100_000, 130_000, false)).toBe(true);
  });
});
