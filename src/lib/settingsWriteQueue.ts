export function enqueueSettingsWrite<T>(queue: { current: Promise<unknown> }, write: () => Promise<T>, guard?: {
  admit: () => boolean;
  skipped: () => T;
}): Promise<T> {
  // Admission belongs to this queue slot, after all preceding writes.
  const result = queue.current.then(() => guard && !guard.admit() ? guard.skipped() : write());
  // A failed write must not poison later independent writes, and is never replayed.
  queue.current = result.catch(() => {});
  return result;
}
