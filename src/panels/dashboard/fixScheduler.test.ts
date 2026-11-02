import { expect, test } from "bun:test";
import { releasePackageOperation, runQueuedPackageOperation, tryAcquirePackageOperation } from "../../lib/packageOperationLock";
import { dashboardFixDependencies, isDashboardPackageFinding, runDashboardFixes, type DashboardFixStep } from "./fixScheduler";

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>(done => { resolve = done; });
  return { promise, resolve };
}

function step(id: string, fn: () => Promise<unknown> = async () => undefined): DashboardFixStep {
  return { id, label: id, fn };
}

test("Fix All runs three independent fixes together and starts waiting fixes as slots free", async () => {
  const gates = Array.from({ length: 6 }, deferred);
  const threeStarted = deferred();
  const fourthStarted = deferred();
  const started: number[] = [];
  let active = 0;
  let maximum = 0;
  const execution = runDashboardFixes(gates.map((_, index) => step(`setting-${index}`)), async (_, index) => {
    active++;
    maximum = Math.max(maximum, active);
    started.push(index);
    if (started.length === 3) threeStarted.resolve();
    if (started.length === 4) fourthStarted.resolve();
    await gates[index].promise;
    active--;
  });
  await threeStarted.promise;
  expect(started).toEqual([0, 1, 2]);
  gates[1].resolve();
  await fourthStarted.promise;
  expect(started).toEqual([0, 1, 2, 3]);
  gates.forEach(gate => gate.resolve());
  await execution;
  expect(started).toEqual([0, 1, 2, 3, 4, 5]);
  expect(maximum).toBe(3);
});

test("a package queue wait leaves independent fixes free to run and installers remain exclusive", async () => {
  expect(tryAcquirePackageOperation()).toBe(true);
  const independentFinished = deferred();
  const order: string[] = [];
  let installers = 0;
  let maximumInstallers = 0;
  const execution = runDashboardFixes([
    step("app-update:one"), step("app-update:two"), step("dependency:vcredist"), step("setting"),
  ], async current => {
    if (!isDashboardPackageFinding(current.id)) {
      order.push(current.id);
      independentFinished.resolve();
      return;
    }
    await runQueuedPackageOperation(async () => {
      installers++;
      maximumInstallers = Math.max(maximumInstallers, installers);
      order.push(current.id);
      await Promise.resolve();
      installers--;
    });
  });
  try {
    await independentFinished.promise;
    expect(order).toEqual(["setting"]);
  } finally {
    releasePackageOperation();
  }
  await execution;
  expect(order).toEqual(["setting", "app-update:one", "app-update:two", "dependency:vcredist"]);
  expect(maximumInstallers).toBe(1);
  expect(tryAcquirePackageOperation()).toBe(true);
  releasePackageOperation();
});

test("failed fixes do not prevent other fixes or cleanup from settling", async () => {
  const order: string[] = [];
  await runDashboardFixes([step("bad"), step("good"), step("disk-cleanup")], async current => {
    order.push(current.id);
    if (current.id === "bad") throw new Error("Windows refused the change");
  });
  expect(order).toEqual(["bad", "good", "disk-cleanup"]);
});

test("prerequisites run before users, browser hardening follows installers, and cleanup is exclusive", async () => {
  const steps = [
    step("disk-cleanup"), step("browser-hardening:edge"), step("app-update:edge"),
    step("setting"), step("services-profile"), step("dependency:powershell7"),
    step("dependency:winget"), step("wincommander-update"), step("pro-sidecar"),
  ];
  const order: string[] = [];
  const active = new Set<string>();
  const exclusiveViolations: string[] = [];
  await runDashboardFixes(steps, async current => {
    if (["disk-cleanup", "services-profile", "wincommander-update", "pro-sidecar"].includes(current.id)) {
      if (active.size) exclusiveViolations.push(current.id);
    }
    active.add(current.id);
    await Promise.resolve();
    order.push(current.id);
    active.delete(current.id);
  });
  expect(exclusiveViolations).toEqual([]);
  const before = (first: string, second: string) => expect(order.indexOf(first) < order.indexOf(second)).toBe(true);
  before("dependency:winget", "dependency:powershell7");
  before("app-update:edge", "browser-hardening:edge");
  before("browser-hardening:edge", "disk-cleanup");
  before("disk-cleanup", "services-profile");
  before("services-profile", "wincommander-update");
  before("wincommander-update", "pro-sidecar");
});

test("missing package tools do not delay settings and failed install attempts still require a fresh capability check", async () => {
  const wingetGate = deferred();
  const settingFinished = deferred();
  const order: string[] = [];
  const wingetAvailable = false;
  const execution = runDashboardFixes([
    step("dependency:winget"), step("dependency:powershell7"), step("setting"), step("app-update:one"),
  ], async current => {
    if (current.id === "dependency:winget") {
      order.push("install attempted");
      await wingetGate.promise;
      throw new Error("WinGet installation failed");
    }
    if (current.id === "setting") {
      order.push("setting fixed");
      settingFinished.resolve();
      return;
    }
    order.push(wingetAvailable ? "capability available" : "capability unavailable");
  });
  await settingFinished.promise;
  expect(order).toEqual(["install attempted", "setting fixed"]);
  wingetGate.resolve();
  await execution;
  expect(order).toEqual(["install attempted", "setting fixed", "capability unavailable", "capability unavailable"]);
});

test("only package-backed dependency findings enter the package queue", () => {
  expect(isDashboardPackageFinding("dependency:powershell7")).toBe(true);
  expect(isDashboardPackageFinding("app-update:Microsoft.Edge")).toBe(true);
  expect(isDashboardPackageFinding("dependency:ramDiskEngine")).toBe(false);
  expect(isDashboardPackageFinding("telemetry-blocklist")).toBe(false);
  expect(dashboardFixDependencies([step("browser-hardening:edge"), step("dependency:ramDiskEngine")])[0]).toEqual([1]);
});
