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

// KT: Tweaks panels/tabs unmount, so state must live outside component state
// to prevent navigation from silently triggering fresh work / losing the
// active tab.
export function useTweaksSessionState<T>(
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
