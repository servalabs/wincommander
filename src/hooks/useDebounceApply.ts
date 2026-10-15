import { useCallback, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { createDebouncedApply, type ApplyStatus } from '../lib/debouncedApply';

export function useDebounceApply(delayMs = 5000) {
  const [{ status, secsLeft }, setState] = useState<{ status: ApplyStatus; secsLeft: number }>({ status: 'idle', secsLeft: 0 });
  const createOwner = useCallback((delay: number) => ({
    delay, disposed: false,
    controller: createDebouncedApply(delay, (status, secsLeft) => setState({ status, secsLeft })),
  }), []);
  const ownerRef = useRef<ReturnType<typeof createOwner> | null>(null);
  // The first committed render can accept a user action before passive effects.
  if (ownerRef.current === null) ownerRef.current = createOwner(delayMs);
  useLayoutEffect(() => {
    let owner = ownerRef.current!;
    if (owner.disposed || owner.delay !== delayMs) {
      owner.controller.dispose();
      owner = createOwner(delayMs);
      ownerRef.current = owner;
      setState({ status: 'idle', secsLeft: 0 });
    }
    return () => { owner.disposed = true; owner.controller.dispose(); };
  }, [delayMs, createOwner]);
  const schedule = useCallback((apply: () => Promise<boolean>) => ownerRef.current?.controller.schedule(apply), []);
  const cancel = useCallback(() => ownerRef.current?.controller.cancel(), []);
  return useMemo(() => ({
    schedule, cancel, status, secsLeft,
  }), [schedule, cancel, status, secsLeft]);
}
