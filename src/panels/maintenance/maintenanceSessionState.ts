import {
  useCallback,
  useRef,
  useSyncExternalStore,
  type Dispatch,
  type SetStateAction,
} from "react";

const sessionValues = new Map<string, unknown>();
const sessionListeners = new Map<string, Set<() => void>>();

function readSessionValue<T>(key: string, initialValue: T): T {
  if (!sessionValues.has(key)) sessionValues.set(key, initialValue);
  return sessionValues.get(key) as T;
}

export function getMaintenanceSessionValue<T>(key: string): T | undefined {
  return sessionValues.get(key) as T | undefined;
}

// Panel-level preloads complete before their tab hooks mount. Publishing through
// the same store keeps those hooks cache-only and avoids a second scan on entry.
export function primeMaintenanceSessionValue<T>(key: string, value: T): void {
  sessionValues.set(key, value);
  sessionListeners.get(key)?.forEach((listener) => listener());
}

// KT: Maintenance tabs and panels unmount, so scan previews must live outside
// component state to prevent navigation from silently triggering fresh work.
export function useMaintenanceSessionState<T>(
  key: string,
  initialValue: T,
): [T, Dispatch<SetStateAction<T>>] {
  const initialValueRef = useRef(initialValue);
  const subscribe = useCallback((listener: () => void) => {
    const listeners = sessionListeners.get(key) ?? new Set<() => void>();
    listeners.add(listener);
    sessionListeners.set(key, listeners);
    return () => {
      listeners.delete(listener);
      if (!listeners.size) sessionListeners.delete(key);
    };
  }, [key]);

  const getSnapshot = useCallback(
    () => readSessionValue(key, initialValueRef.current),
    [key],
  );
  const value = useSyncExternalStore(subscribe, getSnapshot, getSnapshot);

  const setValue: Dispatch<SetStateAction<T>> = useCallback((nextValue) => {
    const current = readSessionValue(key, initialValueRef.current);
    const resolved = typeof nextValue === "function"
      ? (nextValue as (previous: T) => T)(current)
      : nextValue;
    sessionValues.set(key, resolved);
    sessionListeners.get(key)?.forEach((listener) => listener());
  }, [key]);

  return [value, setValue];
}
