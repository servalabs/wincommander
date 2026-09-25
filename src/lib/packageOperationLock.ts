/**
 * A process-wide mutex for package-manager work.  Winget and the other
 * managers are also used by legacy update surfaces, so ID-level queues alone
 * cannot prevent two different update screens from starting competing work.
 */
let packageOperationInFlight = false;
const packageOperationWaiters: Array<() => void> = [];
const PACKAGE_BACKED_DEPENDENCY_IDS = new Set([
  "meshVpn",
  "productivityEngine",
  "winget",
  "powershell7",
  "vcredist",
  "privacyShieldAI",
  "systemCleaner",
  "instantSearch",
  "diskHealthEngine",
  "metadataScrubber",
  "localLlm",
]);

export function isPackageBackedDependency(dependencyId: string): boolean {
  return PACKAGE_BACKED_DEPENDENCY_IDS.has(dependencyId);
}

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

/** Run package-manager work in FIFO order and always hand the lock onward. */
export async function runQueuedPackageOperation<T>(
  operation: (wasQueued: boolean) => Promise<T>,
  onQueued?: () => void,
  beforeRelease?: () => void,
): Promise<T> {
  const wasQueued = packageOperationInFlight;
  if (wasQueued) onQueued?.();
  await waitForPackageOperation();
  try {
    return await operation(wasQueued);
  } finally {
    try {
      beforeRelease?.();
    } finally {
      releasePackageOperation();
    }
  }
}

/** Serialize dependency installers that invoke WinGet with app installs and updates. */
export function runQueuedDependencyInstall<T>(
  dependencyId: string,
  operation: () => Promise<T>,
  onQueued?: () => void,
): Promise<T> {
  if (!isPackageBackedDependency(dependencyId)) return operation();
  return runQueuedPackageOperation(() => operation(), onQueued);
}
