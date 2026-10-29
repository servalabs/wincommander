import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { waitForSoftTimeout } from "./softTimeout";
import { joinInventoryScan, verifySavedInventory } from "./appInventoryScanResult";

test("accepts only the exact canonical saved inventory regardless of object key order", async () => {
  const saved = { lastScanAt: "2026-10-28T12:00:00Z", manifestApps: [{ id: "app", installed: true }], summary: { total: 1 } };
  const data = { ...saved, __wcInventoryReceipt: { state: "confirmed", sha256: "2b2bb33f21608da3fd5d456c724aed9e158c99b013c4d5156ee2fbdf5922ce27" } };
  expect(await verifySavedInventory(data, { summary: { total: 1 }, manifestApps: [{ installed: true, id: "app" }], lastScanAt: saved.lastScanAt })).toBe(true);
  expect(await verifySavedInventory(data, { ...saved, manifestApps: [{ id: "other", installed: true }] })).toBe(false);
  expect(await verifySavedInventory(data, { ...saved, summary: { total: 0 } })).toBe(false);
  expect(await verifySavedInventory(data, null)).toBe(false);
});

test("missing, failed or malformed receipts cannot confirm saved state", async () => {
  for (const data of [null, {}, { __wcInventoryReceipt: { state: "unconfirmed" } }, { __wcInventoryReceipt: { state: "confirmed", inventory: null } }]) {
    expect(await verifySavedInventory(data, {})).toBe(false);
  }
});


test("matches the native canonical digest for the full inventory fixture", async () => {
  const inventory = JSON.parse(readFileSync("src/lib/appInventoryHashFixture.json", "utf8"));
  const result = { __wcInventoryReceipt: { state: "confirmed", sha256: "8517e4f77bd97442192d51b2f335a538fcc4f45419671963ce418c299902ce05" } };
  expect(await verifySavedInventory(result, inventory)).toBe(true);
  inventory.manifestApps[0].name += "changed";
  expect(await verifySavedInventory(result, inventory)).toBe(false);
});


test("concurrent callers join the same live scan until it actually finishes", async () => {
  const slot: { current: Promise<boolean> | null } = { current: null };
  let resolve!: (value: boolean) => void;
  let starts = 0;
  const start = () => { starts++; return new Promise<boolean>(done => { resolve = done; }); };
  const first = joinInventoryScan(slot, start);
  const second = joinInventoryScan(slot, start);
  expect(first).toBe(second);
  await Promise.resolve();
  expect(starts).toBe(1);
  expect(slot.current).toBe(first);
  resolve(false);
  expect(await first).toBe(false);
  expect(slot.current).toBe(null);
});


test("a soft timeout leaves the shared scan active until the verified waiter drains it", async () => {
  const slot: { current: Promise<boolean> | null } = { current: null };
  let resolve!: (value: boolean) => void;
  let starts = 0;
  const start = () => { starts++; return new Promise<boolean>(done => { resolve = done; }); };
  const task = joinInventoryScan(slot, start);
  expect((await waitForSoftTimeout(task, 5)).status).toBe("timed-out");
  expect(joinInventoryScan(slot, start)).toBe(task);
  expect(starts).toBe(1);
  resolve(true);
  expect(await task).toBe(true);
  expect(slot.current).toBe(null);
});
