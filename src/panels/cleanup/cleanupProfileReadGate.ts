/** Prevent a late account scan from being shown under a different selection. */
export function createCleanupProfileReadGate() {
  let generation = 0;
  return {
    begin: () => ++generation,
    isCurrent: (ticket: number) => ticket === generation,
  };
}
