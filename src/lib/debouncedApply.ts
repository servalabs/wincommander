export type ApplyStatus = 'idle' | 'pending' | 'applying' | 'applied' | 'failed' | 'unknown';
type Clock = Pick<typeof globalThis, 'setTimeout' | 'clearTimeout' | 'setInterval' | 'clearInterval'>;

/** Owns presentation of an apply receipt. Cancellation cannot undo native work. */
export function createDebouncedApply(delayMs: number, publish: (status: ApplyStatus, seconds: number) => void, clock: Clock = globalThis) {
  let generation = 0;
  let disposed = false;
  let running = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let tick: ReturnType<typeof setInterval> | undefined;
  let reset: ReturnType<typeof setTimeout> | undefined;
  const clear = () => {
    if (timer !== undefined) clock.clearTimeout(timer);
    if (tick !== undefined) clock.clearInterval(tick);
    if (reset !== undefined) clock.clearTimeout(reset);
    timer = tick = reset = undefined;
  };
  const emit = (version: number, status: ApplyStatus, seconds = 0) => {
    if (!disposed && version === generation) publish(status, seconds);
  };
  return {
    schedule(apply: () => Promise<boolean>) {
      if (disposed) return;
      const version = ++generation;
      clear();
      let seconds = Math.ceil(delayMs / 1000);
      emit(version, 'pending', seconds);
      tick = clock.setInterval(() => emit(version, 'pending', seconds = Math.max(0, seconds - 1)), 1000);
      timer = clock.setTimeout(async () => {
        clear();
        running++;
        emit(version, 'applying');
        let succeeded = false;
        try { succeeded = await apply() === true; } catch { /* An exception is never a success receipt. */ }
        finally { running--; }
        if (disposed || version !== generation) return;
        emit(version, succeeded ? 'applied' : 'failed');
        if (succeeded) reset = clock.setTimeout(() => emit(version, 'idle'), 2000);
      }, delayMs);
    },
    cancel() {
      clear();
      emit(++generation, running ? 'unknown' : 'idle');
    },
    dispose() { disposed = true; generation++; clear(); },
  };
}
