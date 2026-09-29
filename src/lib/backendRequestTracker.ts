export function createBackendRequestTracker() {
  const pending = new Map<string, Promise<unknown>>();
  return async function run<T>(command: string, key: string, start: () => Promise<T>): Promise<T> {
    // Vault coordinates timer reads itself; explicit refresh must not inherit an older probe.
    if (command === "Get-EncryptedVolumeStatus") return start();
    const existing = pending.get(key) as Promise<T> | undefined;
    if (existing) return existing;
    const request = start();
    pending.set(key, request);
    try {
      return await request;
    } finally {
      pending.delete(key);
    }
  };
}
