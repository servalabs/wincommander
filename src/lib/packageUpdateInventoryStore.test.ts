import { describe, expect, test } from "bun:test";
import type { PackageUpdateInventory } from "../hooks/useBackend";
import {
  getPackageUpdateInventorySnapshot,
  runPackageUpdateInventoryCheck,
} from "./packageUpdateInventoryStore";

describe("package update inventory store", () => {
  test("shares an in-flight startup or refresh check", async () => {
    let release!: (value: PackageUpdateInventory) => void;
    let calls = 0;
    const inventory: PackageUpdateInventory = { managers: [], cancelled: false };
    const first = runPackageUpdateInventoryCheck(() => {
      calls += 1;
      return new Promise((resolve) => { release = resolve; });
    });
    const second = runPackageUpdateInventoryCheck(async () => {
      calls += 1;
      return inventory;
    });

    expect(second).toBe(first);
    expect(getPackageUpdateInventorySnapshot().status).toBe("checking");
    await Promise.resolve();
    expect(calls).toBe(1);
    release(inventory);
    expect(await first).toBe(inventory);
    expect(calls).toBe(1);
    expect(getPackageUpdateInventorySnapshot()).toMatchObject({
      status: "ready",
      inventory,
      error: null,
    });
  });
});
