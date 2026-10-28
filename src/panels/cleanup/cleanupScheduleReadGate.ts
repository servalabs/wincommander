/** Keeps a slower Task Scheduler read from overwriting a newer readback. */
export function createCleanupScheduleReadGate() {
  let generation = 0;
  return {
    begin: () => ++generation,
    isCurrent: (ticket: number) => ticket === generation,
  };
}
