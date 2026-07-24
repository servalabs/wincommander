import { afterEach, expect, test } from "bun:test";
import { releasePackageOperation, tryAcquirePackageOperation } from "./packageOperationLock";

afterEach(() => releasePackageOperation());

test("package-manager work is exclusive until released", () => {
  expect(tryAcquirePackageOperation()).toBe(true);
  expect(tryAcquirePackageOperation()).toBe(false);
  releasePackageOperation();
  expect(tryAcquirePackageOperation()).toBe(true);
});
