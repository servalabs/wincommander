import { afterEach, expect, test } from "bun:test";
import { isPackageBackedDependency, releasePackageOperation, runQueuedDependencyInstall, runQueuedPackageOperation, tryAcquirePackageOperation, waitForPackageOperation } from "./packageOperationLock";

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

test("queued work runs after the current operation and releases the lock after failure", async () => {
  expect(tryAcquirePackageOperation()).toBe(true);
  const order: string[] = [];
  let showedQueuedState = false;
  const first = runQueuedPackageOperation(async (wasQueued) => {
    expect(wasQueued).toBe(true);
    order.push("first");
    throw new Error("operation failed");
  }, () => { showedQueuedState = true; }, () => order.push("release cleanup"));
  const second = runQueuedPackageOperation(async (wasQueued) => {
    expect(wasQueued).toBe(true);
    order.push("second");
  });

  expect(showedQueuedState).toBe(true);
  expect(order).toEqual([]);
  releasePackageOperation();

  let failure: unknown;
  try {
    await first;
  } catch (error) {
    failure = error;
  }
  expect(failure instanceof Error).toBe(true);
  expect((failure as Error).message).toBe("operation failed");
  await second;
  expect(order).toEqual(["first", "release cleanup", "second"]);
  expect(tryAcquirePackageOperation()).toBe(true);
});

test("PowerShell and other package-backed dependency installs wait in the package FIFO", async () => {
  for (const id of [
    "meshVpn", "productivityEngine", "winget", "powershell7", "vcredist",
    "privacyShieldAI", "systemCleaner", "instantSearch", "diskHealthEngine",
    "metadataScrubber", "localLlm",
  ]) {
    expect(isPackageBackedDependency(id)).toBe(true);
  }
  expect(tryAcquirePackageOperation()).toBe(true);

  const order: string[] = [];
  let showedQueuedState = false;
  const install = runQueuedDependencyInstall("powershell7", async () => {
    order.push("PowerShell 7 install");
  }, () => { showedQueuedState = true; });

  expect(showedQueuedState).toBe(true);
  expect(order).toEqual([]);
  releasePackageOperation();
  await install;
  expect(order).toEqual(["PowerShell 7 install"]);
  expect(tryAcquirePackageOperation()).toBe(true);
});

test("non-package dependency installs do not block service work behind the package lock", async () => {
  expect(isPackageBackedDependency("ramDiskEngine")).toBe(false);
  expect(tryAcquirePackageOperation()).toBe(true);

  let directInstallerRan = false;
  await runQueuedDependencyInstall("ramDiskEngine", async () => {
    directInstallerRan = true;
  });

  expect(directInstallerRan).toBe(true);
  expect(tryAcquirePackageOperation()).toBe(false);
  releasePackageOperation();
  expect(tryAcquirePackageOperation()).toBe(true);
});
