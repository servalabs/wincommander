/** An older observation must not restore a mount removed by a newer refresh. */
export function createVaultStatusRefresh<T>(publish: (value: T | null) => void, setRefreshing: (value: boolean) => void = () => {}) {
  let latestRequest = 0;
  let pending = 0;
  return async (read: () => Promise<T | null>): Promise<T | null> => {
    const request = ++latestRequest;
    if (++pending === 1) setRefreshing(true);
    try {
      let value: T | null;
      try { value = await read(); } catch { value = null; }
      if (request === latestRequest) publish(value);
      return value;
    } finally {
      if (--pending === 0) setRefreshing(false);
    }
  };
}
