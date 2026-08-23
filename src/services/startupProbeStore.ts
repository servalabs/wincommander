export interface StartupProbeSnapshot<T> {
  value: T | null;
  refreshedAt: number | null;
  isRefreshing: boolean;
}

type ProbeListener = () => void;

/**
 * Holds the single authoritative result of a startup probe. Consumers can
 * subscribe instead of each issuing the same native probe while launch is busy.
 */
export interface StartupProbeStore<T> {
  getSnapshot(): StartupProbeSnapshot<T>;
  subscribe(listener: ProbeListener): () => void;
  refresh(
    load: (signal: AbortSignal) => Promise<T>,
    signal: AbortSignal,
  ): Promise<T>;
}

export function createStartupProbeStore<T>(
  now: () => number = Date.now,
): StartupProbeStore<T> {
  let snapshot: StartupProbeSnapshot<T> = {
    value: null,
    refreshedAt: null,
    isRefreshing: false,
  };
  let inFlight: Promise<T> | null = null;
  const listeners = new Set<ProbeListener>();

  const publish = () => listeners.forEach((listener) => listener());

  return {
    getSnapshot: () => snapshot,
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    refresh(load, signal) {
      if (inFlight) return inFlight;
      snapshot = { ...snapshot, isRefreshing: true };
      publish();
      inFlight = load(signal)
        .then((value) => {
          if (!signal.aborted) {
            snapshot = { value, refreshedAt: now(), isRefreshing: false };
            publish();
          }
          return value;
        })
        .finally(() => {
          inFlight = null;
          if (snapshot.isRefreshing) {
            snapshot = { ...snapshot, isRefreshing: false };
            publish();
          }
        });
      return inFlight;
    },
  };
}
