/**
 * A process-wide mutex for package-manager work.  Winget and the other
 * managers are also used by legacy update surfaces, so ID-level queues alone
 * cannot prevent two different update screens from starting competing work.
 */
let packageOperationInFlight = false;

export function tryAcquirePackageOperation(): boolean {
  if (packageOperationInFlight) return false;
  packageOperationInFlight = true;
  return true;
}

export function releasePackageOperation(): void {
  packageOperationInFlight = false;
}
