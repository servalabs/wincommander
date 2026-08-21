/**
 * A process-wide mutex for package-manager work.  Winget and the other
 * managers are also used by legacy update surfaces, so ID-level queues alone
 * cannot prevent two different update screens from starting competing work.
 */
let packageOperationInFlight = false;
const packageOperationWaiters: Array<() => void> = [];

export function tryAcquirePackageOperation(): boolean {
  if (packageOperationInFlight) return false;
  packageOperationInFlight = true;
  return true;
}

/**
 * Join the package-manager FIFO. Unlike `tryAcquirePackageOperation`, this
 * never rejects work merely because Winget is busy: callers wait their turn
 * and are handed the lock in arrival order.
 */
export function waitForPackageOperation(): Promise<void> {
  if (!packageOperationInFlight) {
    packageOperationInFlight = true;
    return Promise.resolve();
  }
  return new Promise((resolve) => packageOperationWaiters.push(resolve));
}

export function releasePackageOperation(): void {
  const next = packageOperationWaiters.shift();
  if (next) {
    // Keep the lock held while ownership moves to the next queued task, so a
    // newly-clicked package action cannot jump the FIFO between jobs.
    next();
    return;
  }
  packageOperationInFlight = false;
}
