export type PanelImport = () => Promise<unknown>;

type QueueItem = {
  id: string;
  load: PanelImport;
};

/**
 * Keeps optional panels off the launch path while still making deliberate
 * navigation feel immediate. Imports already running cannot be cancelled by
 * the browser, so a new navigation removes only obsolete queued work.
 */
export class PanelPrefetchQueue {
  private readonly queued = new Map<string, QueueItem>();
  private readonly inFlight = new Set<string>();
  private readonly completed = new Set<string>();
  private active = 0;
  private disposed = false;

  constructor(private readonly maxConcurrent = 1) {}

  enqueueIntent(id: string, load: PanelImport): void {
    if (this.disposed || this.queued.has(id) || this.inFlight.has(id) || this.completed.has(id)) return;
    this.queued.delete(id);
    this.queued.set(id, { id, load });
    this.drain();
  }

  enqueueIdle(items: readonly QueueItem[]): void {
    if (this.disposed) return;
    for (const item of items) {
      if (!this.queued.has(item.id) && !this.inFlight.has(item.id) && !this.completed.has(item.id)) {
        this.queued.set(item.id, item);
      }
    }
    this.drain();
  }

  keepRelevant(id: string): void {
    for (const queuedId of this.queued.keys()) {
      if (queuedId !== id) this.queued.delete(queuedId);
    }
  }

  dispose(): void {
    this.disposed = true;
    this.queued.clear();
  }

  private drain(): void {
    while (!this.disposed && this.active < this.maxConcurrent) {
      const next = this.queued.entries().next().value as [string, QueueItem] | undefined;
      if (!next) return;
      this.queued.delete(next[0]);
      this.active += 1;
      this.inFlight.add(next[0]);
      void next[1].load()
        .then(() => { this.completed.add(next[0]); })
        .catch(() => undefined)
        .finally(() => {
          this.active -= 1;
          this.inFlight.delete(next[0]);
          this.drain();
        });
    }
  }
}

export function scheduleWhenIdle(callback: () => void): () => void {
  const idleWindow = window as Window & {
    requestIdleCallback?: (handler: () => void, options?: { timeout: number }) => number;
    cancelIdleCallback?: (handle: number) => void;
  };

  if (idleWindow.requestIdleCallback) {
    const handle = idleWindow.requestIdleCallback(callback, { timeout: 10_000 });
    return () => idleWindow.cancelIdleCallback?.(handle);
  }

  const handle = window.setTimeout(callback, 2_000);
  return () => window.clearTimeout(handle);
}
