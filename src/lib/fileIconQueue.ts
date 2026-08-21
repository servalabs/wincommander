// SPDX-License-Identifier: AGPL-3.0-or-later
// A small shared queue prevents a large result set from turning native shell
// icon lookup into an unbounded IPC burst.

export const MAX_CONCURRENT_FILE_ICON_REQUESTS = 8;

type IconData = string | null;
type IconListener = (data: IconData) => void;
type IconLoader = (path: string) => Promise<IconData>;

interface PendingIcon {
  path: string;
  priority: number;
  listeners: Set<IconListener>;
  running: boolean;
}

export class FileIconQueue {
  private readonly cache = new Map<string, IconData>();
  private readonly pending = new Map<string, PendingIcon>();
  private activeRequests = 0;
  private pumpScheduled = false;

  constructor(
    private readonly loadIcon: IconLoader,
    private readonly concurrency = MAX_CONCURRENT_FILE_ICON_REQUESTS,
  ) {}

  get(path: string): IconData | undefined {
    return this.cache.get(path);
  }

  prime(path: string, data: IconData) {
    this.cache.set(path, data);
  }

  request(path: string, priority: number, listener: IconListener): () => void {
    const cached = this.cache.get(path);
    if (cached !== undefined || this.cache.has(path)) {
      listener(cached ?? null);
      return () => {};
    }

    let pending = this.pending.get(path);
    if (!pending) {
      pending = { path, priority, listeners: new Set(), running: false };
      this.pending.set(path, pending);
    } else {
      pending.priority = Math.min(pending.priority, priority);
    }
    pending.listeners.add(listener);
    this.schedulePump();

    return () => {
      pending?.listeners.delete(listener);
      if (pending && !pending.running && pending.listeners.size === 0) {
        this.pending.delete(path);
      }
    };
  }

  private schedulePump() {
    if (this.pumpScheduled) return;
    this.pumpScheduled = true;
    queueMicrotask(() => {
      this.pumpScheduled = false;
      this.pump();
    });
  }

  private pump() {
    while (this.activeRequests < this.concurrency) {
      const next = [...this.pending.values()]
        .filter((entry) => !entry.running && entry.listeners.size > 0)
        .sort((a, b) => a.priority - b.priority)[0];
      if (!next) return;

      next.running = true;
      this.activeRequests += 1;
      this.loadIcon(next.path)
        .catch(() => null)
        .then((data) => {
          this.cache.set(next.path, data);
          for (const listener of next.listeners) listener(data);
        })
        .finally(() => {
          this.pending.delete(next.path);
          this.activeRequests -= 1;
          this.pump();
        });
    }
  }
}
