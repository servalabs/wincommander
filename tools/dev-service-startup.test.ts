import { expect, test } from "bun:test";
import { syncDevelopmentService } from "./dev-service-startup";

test("Free development synchronizes the settings service without requiring Pro", async () => {
  const calls: string[][] = [];
  await syncDevelopmentService(true, async args => { calls.push(args); return 0; });
  expect(calls).toHaveLength(1);
  expect(calls[0]).toContain("tools/sync-dev-service.ps1");
  expect(calls[0]).not.toContain("-SyncPro");
});

test("Pro development synchronizes both service and Pro", async () => {
  const calls: string[][] = [];
  await syncDevelopmentService(false, async args => { calls.push(args); return 0; });
  expect(calls).toHaveLength(1);
  expect(calls[0]).toContain("-SyncPro");
});

test("both modes stop startup if service synchronization fails", async () => {
  for (const freeOnly of [true, false]) {
    await expect(syncDevelopmentService(freeOnly, async () => 5))
      .rejects.toThrow("Vite was not started");
    await expect(syncDevelopmentService(freeOnly, async () => { throw new Error("cancelled"); }))
      .rejects.toThrow("cancelled");
  }
});
