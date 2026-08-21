import { afterEach, expect, test } from "bun:test";
import { releasePackageOperation, tryAcquirePackageOperation, waitForPackageOperation } from "./packageOperationLock";

afterEach(() => releasePackageOperation());

test("package-manager work is exclusive until released", () => {
  expect(tryAcquirePackageOperation()).toBe(true);
  expect(tryAcquirePackageOperation()).toBe(false);
  releasePackageOperation();
  expect(tryAcquirePackageOperation()).toBe(true);
});

test("queued package work receives the lock in FIFO order", async () => {
  expect(tryAcquirePackageOperation()).toBe(true);
  const order: string[] = [];
  const first = waitForPackageOperation().then(() => order.push("first"));
  const second = waitForPackageOperation().then(() => order.push("second"));

  releasePackageOperation();
  await first;
  expect(order).toEqual(["first"]);
  expect(tryAcquirePackageOperation()).toBe(false);

  releasePackageOperation();
  await second;
  expect(order).toEqual(["first", "second"]);
});
