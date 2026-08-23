import {
  dispatchStartupMilestone,
  type StartupJobId,
  type StartupMilestone,
  type StartupNativeReporter,
} from "../events/startup";

export type StartupPriority = "critical" | "background" | "idle";
export type StartupCost = "light" | "expensive";
export type StartupJobOutcome = Exclude<StartupMilestone, "queued" | "started">;

export interface StartupJob<T = unknown> {
  id: StartupJobId;
  priority: StartupPriority;
  cost: StartupCost;
  timeoutMs: number;
  run(signal: AbortSignal): Promise<T>;
}

export interface StartupJobResult<T = unknown> {
  id: StartupJobId;
  outcome: StartupJobOutcome;
  value?: T;
}

export interface StartupCoordinator {
  run<T>(job: StartupJob<T>): Promise<StartupJobResult<T>>;
  runBatch(jobs: readonly StartupJob[]): Promise<StartupJobResult[]>;
  cancel(): void;
}

export interface StartupCoordinatorOptions {
  now?: () => number;
  reportToNative?: StartupNativeReporter;
}

interface StartupFlight {
  result: Promise<StartupJobResult>;
  drain: Promise<void>;
}

const PRIORITY_ORDER: Record<StartupPriority, number> = {
  critical: 0,
  background: 1,
  idle: 2,
};

function report(
  reporter: StartupNativeReporter | undefined,
  job: StartupJobId,
  milestone: StartupMilestone,
  durationMs: number,
): void {
  const event = { job, milestone, durationMs };
  dispatchStartupMilestone(event);
  void reporter?.report(event).catch(() => {
    // Startup telemetry is diagnostic only and must never delay the shell.
  });
}

/**
 * Coordinates launch work without pretending that a frontend timeout can kill
 * native work. Timed-out and cancelled results are ignored; the next expensive
 * job still waits for the original promise to settle, keeping one costly probe
 * active at a time.
 */
export function createStartupCoordinator(
  options: StartupCoordinatorOptions = {},
): StartupCoordinator {
  const now = options.now ?? Date.now;
  const controller = new AbortController();
  const flights = new Map<StartupJobId, StartupFlight>();
  let expensiveTail: Promise<void> = Promise.resolve();

  const execute = <T>(job: StartupJob<T>) => {
    const startedAt = now();
    report(options.reportToNative, job.id, "started", 0);
    if (controller.signal.aborted) {
      report(options.reportToNative, job.id, "cancelled", 0);
      return {
        result: Promise.resolve<StartupJobResult<T>>({
          id: job.id,
          outcome: "cancelled",
        }),
        drain: Promise.resolve(),
      };
    }

    const jobController = new AbortController();
    const abortJob = () => jobController.abort();
    controller.signal.addEventListener("abort", abortJob, { once: true });
    let timeout: ReturnType<typeof setTimeout> | undefined;
    const operation = Promise.resolve().then(() =>
      job.run(jobController.signal),
    );
    const drain = operation.then(
      () => undefined,
      () => undefined,
    );
    const timedOut = new Promise<"timed-out">((resolve) => {
      timeout = setTimeout(() => {
        abortJob();
        resolve("timed-out");
      }, job.timeoutMs);
    });
    const cancelled = new Promise<"cancelled">((resolve) => {
      controller.signal.addEventListener("abort", () => resolve("cancelled"), {
        once: true,
      });
    });

    const result = (async (): Promise<StartupJobResult<T>> => {
      try {
        const settled = await Promise.race([operation, timedOut, cancelled]);
        const durationMs = now() - startedAt;
        if (settled === "timed-out") {
          operation.catch(() => {});
          report(options.reportToNative, job.id, "timed-out", durationMs);
          return { id: job.id, outcome: "timed-out" };
        }
        if (settled === "cancelled") {
          operation.catch(() => {});
          report(options.reportToNative, job.id, "cancelled", durationMs);
          return { id: job.id, outcome: "cancelled" };
        }
        report(options.reportToNative, job.id, "completed", durationMs);
        return { id: job.id, outcome: "completed", value: settled };
      } catch {
        report(options.reportToNative, job.id, "failed", now() - startedAt);
        return { id: job.id, outcome: "failed" };
      } finally {
        if (timeout) clearTimeout(timeout);
        controller.signal.removeEventListener("abort", abortJob);
      }
    })();
    return { result, drain };
  };

  const run = <T>(job: StartupJob<T>): Promise<StartupJobResult<T>> => {
    const existing = flights.get(job.id) as StartupFlight | undefined;
    if (existing) return existing.result as Promise<StartupJobResult<T>>;

    report(options.reportToNative, job.id, "queued", 0);

    let scheduled: Promise<StartupJobResult<T>>;
    let drain: Promise<void>;
    if (job.cost === "expensive") {
      const previousExpensive = expensiveTail;
      let releaseExpensive!: () => void;
      expensiveTail = new Promise<void>((resolve) => {
        releaseExpensive = resolve;
      });
      drain = expensiveTail;
      scheduled = previousExpensive.then(() => {
        const execution = execute(job);
        void execution.drain.finally(releaseExpensive);
        return execution.result;
      });
    } else {
      const execution = execute(job);
      scheduled = execution.result;
      drain = execution.drain;
    }
    const flight: StartupFlight = { result: scheduled, drain };
    flights.set(job.id, flight);
    void drain.finally(() => {
      if (flights.get(job.id) === flight) flights.delete(job.id);
    });
    return scheduled;
  };

  return {
    run,
    async runBatch(jobs) {
      const ordered = [...jobs].sort(
        (left, right) =>
          PRIORITY_ORDER[left.priority] - PRIORITY_ORDER[right.priority],
      );
      return Promise.all(ordered.map((job) => run(job)));
    },
    cancel() {
      controller.abort();
    },
  };
}
