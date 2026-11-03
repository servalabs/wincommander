import { expect, test } from "bun:test";
import { shouldReconcileCalculatorShortcuts, syncCalculatorShortcuts } from "./calculatorShortcuts";

test("reports shortcut failures even when the backend resolves normally", async () => {
  expect(await syncCalculatorShortcuts(async () => ({ success: false, error: "Access denied" }), true)).toBe("Access denied");
  expect(await syncCalculatorShortcuts(async () => ({ success: true, data: { status: "hidden", itemsChanged: 1, warnings: ["Shortcut unchanged"] } }), true)).toBe("Shortcut unchanged");
});

test("reports success only when shortcut operation succeeded without warnings", async () => {
  expect(await syncCalculatorShortcuts(async () => ({ success: true, data: { status: "hidden", itemsChanged: 1 } }), true)).toBeNull();
  expect(await syncCalculatorShortcuts(async () => { throw new Error("failed"); }, true)).toContain("failed");
});

test("reconciles an armed lock after real unlock, including Hide mode", () => {
  const ready = { unlocked: true, decoy: false, hasRealPin: true, enabled: true, hideMode: false };
  expect(shouldReconcileCalculatorShortcuts(ready)).toBe(true);
  expect(shouldReconcileCalculatorShortcuts({ ...ready, hideMode: true })).toBe(true);
  expect(shouldReconcileCalculatorShortcuts({ ...ready, unlocked: false })).toBe(false);
  expect(shouldReconcileCalculatorShortcuts({ ...ready, decoy: true })).toBe(false);
  expect(shouldReconcileCalculatorShortcuts({ ...ready, hasRealPin: false })).toBe(false);
  expect(shouldReconcileCalculatorShortcuts({ ...ready, enabled: false })).toBe(false);
  expect(shouldReconcileCalculatorShortcuts({ ...ready, enabled: false, hideMode: true })).toBe(false);
});

test("serializes an automatic shortcut update before a later disable", async () => {
  const calls: boolean[] = [];
  let release!: () => void;
  const wait = new Promise<void>((resolve) => { release = resolve; });
  const update = async (hidden: boolean) => {
    calls.push(hidden);
    if (hidden) await wait;
    return { success: true, data: { status: hidden ? "hidden" : "restored", itemsChanged: 0 } };
  };
  const enabled = syncCalculatorShortcuts(update, true);
  const disabled = syncCalculatorShortcuts(update, false);
  await Promise.resolve();
  expect(calls).toEqual([true]);
  release();
  await Promise.all([enabled, disabled]);
  expect(calls).toEqual([true, false]);
});
