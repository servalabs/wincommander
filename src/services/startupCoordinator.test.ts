import { describe, expect, test } from "bun:test";
import { createStartupCoordinator } from "./startupCoordinator";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

describe("startup coordinator", () => {
  test("shares a duplicate job instead of launching it twice", async () => {
    const coordinator = createStartupCoordinator();
    let calls = 0;
    const job = {
      id: "settings-cache" as const,
      priority: "critical" as const,
      cost: "light" as const,
      timeoutMs: 100,
      run: async () => ++calls,
    };

    const [first, second] = await Promise.all([
      coordinator.run(job),
      coordinator.run(job),
    ]);

    expect(calls).toBe(1);
    expect(first).toEqual({
      id: "settings-cache",
      outcome: "completed",
      value: 1,
    });
    expect(second).toEqual(first);
  });

  test("never overlaps expensive probes", async () => {
    const coordinator = createStartupCoordinator();
    const first = deferred<void>();
    const second = deferred<void>();
    const started: string[] = [];

    const firstRun = coordinator.run({
      id: "system-probe",
      priority: "background",
      cost: "expensive",
      timeoutMs: 100,
      run: async () => {
        started.push("first");
        await first.promise;
      },
    });
    const secondRun = coordinator.run({
      id: "startup-status",
      priority: "background",
      cost: "expensive",
      timeoutMs: 100,
      run: async () => {
        started.push("second");
        await second.promise;
      },
    });

    await Promise.resolve();
    await Promise.resolve();
    expect(started).toEqual(["first"]);
    first.resolve();
    await firstRun;
    await Promise.resolve();
    await Promise.resolve();
    expect(started).toEqual(["first", "second"]);
    second.resolve();
    await secondRun;
  });

  test("holds the expensive lane until a timed-out native operation settles", async () => {
    const coordinator = createStartupCoordinator();
    const pending = deferred<string>();
    let calls = 0;
    const result = await coordinator.run({
      id: "system-probe",
      priority: "background",
      cost: "expensive",
      timeoutMs: 5,
      run: async () => {
        calls++;
        return pending.promise;
      },
    });

    expect(result).toEqual({ id: "system-probe", outcome: "timed-out" });
    const duplicate = await coordinator.run({
      id: "system-probe",
      priority: "background",
      cost: "expensive",
      timeoutMs: 100,
      run: async () => {
        calls++;
        return "duplicate";
      },
    });
    expect(duplicate).toEqual({ id: "system-probe", outcome: "timed-out" });
    expect(calls).toBe(1);
    let secondStarted = false;
    const queued = coordinator.run({
      id: "startup-status",
      priority: "background",
      cost: "expensive",
      timeoutMs: 100,
      run: async () => {
        secondStarted = true;
      },
    });
    await Promise.resolve();
    await Promise.resolve();
    expect(secondStarted).toBe(false);
    pending.resolve("late");
    await queued;
    expect(secondStarted).toBe(true);
  });

  test("cancels queued work before it invokes the native operation", async () => {
    const coordinator = createStartupCoordinator();
    const first = deferred<void>();
    let queuedCalls = 0;
    const firstRun = coordinator.run({
      id: "system-probe",
      priority: "background",
      cost: "expensive",
      timeoutMs: 100,
      run: async () => first.promise,
    });
    const queuedRun = coordinator.run({
      id: "startup-status",
      priority: "background",
      cost: "expensive",
      timeoutMs: 100,
      run: async () => {
        queuedCalls++;
      },
    });

    coordinator.cancel();
    first.resolve();
    await Promise.all([firstRun, queuedRun]);

    expect(queuedCalls).toBe(0);
  });
});


test("queued jobs time out without starting after a hung predecessor drains", async () => {
  const coordinator = createStartupCoordinator();
  const blocked = deferred<void>();
  const first = coordinator.run({
    id: "system-probe", priority: "background", cost: "expensive", timeoutMs: 5,
    run: () => blocked.promise,
  });
  await first;
  let calls = 0;
  const queued = await coordinator.run({
    id: "dependencies", priority: "background", cost: "expensive",
    timeoutMs: 100, queueTimeoutMs: 5,
    run: async () => { calls++; },
  });
  expect(queued.outcome).toBe("timed-out");
  blocked.resolve();
  await new Promise(resolve => setTimeout(resolve, 0));
  expect(calls).toBe(0);
});

test("cancellation settles queued callers even if native work never resolves", async () => {
  const coordinator = createStartupCoordinator();
  const blocked = deferred<void>();
  const first = coordinator.run({
    id: "system-probe", priority: "background", cost: "expensive", timeoutMs: 100,
    run: () => blocked.promise,
  });
  await Promise.resolve();
  await Promise.resolve();
  const queued = coordinator.run({
    id: "dependencies", priority: "background", cost: "expensive", timeoutMs: 100,
    run: async () => { throw new Error("must not start"); },
  });
  coordinator.cancel();
  expect((await queued).outcome).toBe("cancelled");
  expect((await first).outcome).toBe("cancelled");
  blocked.resolve();
});
