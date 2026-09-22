// SPDX-License-Identifier: AGPL-3.0-or-later
// Bound reusable icon data independently from queued/in-flight native requests.

export type FileIconData = string | null;
export const MAX_CACHED_FILE_ICONS = 1024;
export const MAX_FILE_ICON_CACHE_BYTES = 8 * 1024 * 1024;

export interface FileIconCacheOptions {
  maxEntries?: number;
  maxBytes?: number;
  ttlMs?: number;
  negativeTtlMs?: number;
  now?: () => number;
}

interface CachedIcon {
  data: FileIconData;
  bytes: number;
  expiresAt: number;
}

export class FileIconCache {
  private readonly entries = new Map<string, CachedIcon>();
  private bytes = 0;
  private readonly maxEntries: number;
  private readonly maxBytes: number;
  private readonly ttlMs: number;
  private readonly negativeTtlMs: number;
  private readonly now: () => number;

  constructor(options: FileIconCacheOptions = {}) {
    this.maxEntries = options.maxEntries ?? MAX_CACHED_FILE_ICONS;
    this.maxBytes = options.maxBytes ?? MAX_FILE_ICON_CACHE_BYTES;
    this.ttlMs = options.ttlMs ?? 5 * 60_000;
    this.negativeTtlMs = options.negativeTtlMs ?? 10_000;
    this.now = options.now ?? (() => performance.now());
    for (const value of [this.maxEntries, this.maxBytes]) {
      if (!Number.isSafeInteger(value) || value < 0) throw new Error("Icon cache budgets must be non-negative safe integers");
    }
    for (const value of [this.ttlMs, this.negativeTtlMs]) {
      if (!Number.isFinite(value) || value < 0) throw new Error("Icon cache lifetimes must be finite and non-negative");
    }
  }

  get(path: string): FileIconData | undefined {
    const entry = this.entries.get(path);
    if (!entry) return undefined;
    if (this.now() >= entry.expiresAt) {
      this.remove(path);
      return undefined;
    }
    this.entries.delete(path);
    this.entries.set(path, entry);
    return entry.data;
  }

  set(path: string, data: FileIconData): void {
    this.remove(path);
    // Conservative UTF-16 string cost, including keys. Map/object overhead is
    // bounded separately by maxEntries; this is not an exact heap-size promise.
    const bytes = 2 * (path.length + (data?.length ?? 0));
    const ttl = data === null ? this.negativeTtlMs : this.ttlMs;
    if (this.maxEntries === 0 || this.maxBytes === 0 || bytes > this.maxBytes || ttl === 0) return;
    const now = this.now();
    if (this.entries.size >= this.maxEntries || this.bytes + bytes > this.maxBytes) {
      // Different lifetimes mean the oldest entry need not be the first to
      // expire. Under pressure, discard expired negatives before live icons.
      for (const [key, entry] of this.entries) {
        if (now >= entry.expiresAt) this.remove(key);
      }
    }
    while (this.entries.size >= this.maxEntries || this.bytes + bytes > this.maxBytes) {
      const oldest = this.entries.keys().next();
      if (oldest.done) break;
      this.remove(oldest.value);
    }
    this.entries.set(path, { data, bytes, expiresAt: now + ttl });
    this.bytes += bytes;
  }

  private remove(path: string): void {
    const entry = this.entries.get(path);
    if (!entry) return;
    this.bytes -= entry.bytes;
    this.entries.delete(path);
  }
}
