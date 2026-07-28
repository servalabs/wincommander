import { beforeEach, describe, expect, test } from "bun:test";
import { clearFrecency, frecencyScore, recordOpen, sortByFrecency, topPaths } from "./frecency";

// Bun does not guarantee a browser-like `localStorage` global, and frecency.ts
// must degrade gracefully whether one exists or not — so every test installs
// its own stand-in rather than relying on whatever bun happens to provide.
// Mirrors the real Storage contract closely enough for frecency.ts's needs.
class MemoryStorage implements Storage {
  private data = new Map<string, string>();

  get length(): number {
    return this.data.size;
  }
  clear(): void {
    this.data.clear();
  }
  getItem(key: string): string | null {
    return this.data.has(key) ? this.data.get(key)! : null;
  }
  key(index: number): string | null {
    return Array.from(this.data.keys())[index] ?? null;
  }
  removeItem(key: string): void {
    this.data.delete(key);
  }
  setItem(key: string, value: string): void {
    this.data.set(key, value);
  }
}

// Simulates a WebView2 privacy mode / quota-exceeded environment: every
// operation throws, the way a real disabled storage does.
class ThrowingStorage implements Storage {
  get length(): number {
    throw new Error("storage disabled");
  }
  clear(): void {
    throw new Error("storage disabled");
  }
  getItem(): string | null {
    throw new Error("storage disabled");
  }
  key(): string | null {
    throw new Error("storage disabled");
  }
  removeItem(): void {
    throw new Error("storage disabled");
  }
  setItem(): void {
    throw new Error("storage disabled");
  }
}

const STORAGE_KEY = "wincmd.frecency.v1";
const DAY_MS = 24 * 60 * 60 * 1000;

let storage: MemoryStorage;

beforeEach(() => {
  storage = new MemoryStorage();
  (globalThis as { localStorage?: Storage }).localStorage = storage;
});

describe("frecencyScore", () => {
  test("a path with no recorded opens scores 0", () => {
    expect(frecencyScore("C:/never/opened.txt")).toBe(0);
  });

  test("more opens ranks higher at equal recency", () => {
    const now = 1_700_000_000_000;
    recordOpen("C:/rare.txt", now);
    for (let i = 0; i < 5; i++) recordOpen("C:/frequent.txt", now);

    expect(frecencyScore("C:/frequent.txt", now)).toBeGreaterThan(frecencyScore("C:/rare.txt", now));
  });

  test("more recent ranks higher at equal open count", () => {
    const now = 1_700_000_000_000;
    recordOpen("C:/stale.txt", now - 60 * DAY_MS); // last opened ~2 months ago
    recordOpen("C:/fresh.txt", now - 1 * DAY_MS); // last opened yesterday

    expect(frecencyScore("C:/fresh.txt", now)).toBeGreaterThan(frecencyScore("C:/stale.txt", now));
  });

  test("30 opens a year ago loses to 2 opens this morning", () => {
    const now = 1_700_000_000_000;
    const yearAgo = now - 365 * DAY_MS;
    for (let i = 0; i < 30; i++) recordOpen("C:/old-habit.exe", yearAgo);
    for (let i = 0; i < 2; i++) recordOpen("C:/todays-tool.exe", now - 60_000); // an hour-ish ago

    expect(frecencyScore("C:/todays-tool.exe", now)).toBeGreaterThan(frecencyScore("C:/old-habit.exe", now));
  });

  test("path keys are normalised — case and separator differences are the same entry", () => {
    const now = 1_700_000_000_000;
    recordOpen("D:/Projects/Notes.TXT", now);

    // Same path, opposite case and separator style, must resolve to the same
    // entry rather than scoring 0 as an unknown path.
    expect(frecencyScore("d:\\projects\\notes.txt", now)).toBeGreaterThan(0);
    expect(frecencyScore("d:\\projects\\notes.txt", now)).toBe(frecencyScore("D:/Projects/Notes.TXT", now));
  });
});

describe("recordOpen / topPaths", () => {
  test("topPaths surfaces the highest scoring paths first", () => {
    const now = 1_700_000_000_000;
    recordOpen("C:/a.txt", now - 30 * DAY_MS);
    for (let i = 0; i < 3; i++) recordOpen("C:/b.txt", now);
    recordOpen("C:/c.txt", now - 200 * DAY_MS);

    expect(topPaths(2, now)).toEqual(["C:/b.txt", "C:/a.txt"]);
  });

  test("eviction at the cap drops the lowest-scoring entry, keeps the rest", () => {
    const base = 1_700_000_000_000;
    let now = base;
    // 501 distinct paths, one per (simulated) day — pushes the store one
    // entry past the ~500 cap.
    for (let i = 0; i <= 500; i++) {
      now = base + i * DAY_MS;
      recordOpen(`C:/history/file${i}.txt`, now);
    }

    const raw = storage.getItem(STORAGE_KEY);
    expect(raw).toBeTruthy();
    const parsed = JSON.parse(raw!) as { entries: Record<string, unknown> };
    expect(Object.keys(parsed.entries)).toHaveLength(500);

    // file0 is both the oldest and lowest-scoring relative to `now` — it must
    // be the one evicted, not an arbitrary entry.
    expect(frecencyScore("C:/history/file0.txt", now)).toBe(0);
    expect(frecencyScore("C:/history/file500.txt", now)).toBeGreaterThan(0);
  });
});

describe("corrupt / hostile storage contents", () => {
  test("truncated, non-JSON storage recovers silently instead of throwing", () => {
    storage.setItem(STORAGE_KEY, "{not json at all");

    expect(frecencyScore("C:/anything.txt")).toBe(0);
    expect(topPaths(5)).toEqual([]);
  });

  test("well-formed JSON with the wrong shape (future version, wrong types) is discarded", () => {
    storage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        v: 99,
        entries: {
          "c:/ok.txt": { path: "C:/ok.txt", opens: 3, lastOpened: 1_700_000_000_000 },
          "c:/bad-opens.txt": { path: "C:/bad-opens.txt", opens: "three", lastOpened: 1_700_000_000_000 },
          "c:/missing-fields.txt": { path: "C:/missing-fields.txt" },
          "c:/null.txt": null,
        },
      }),
    );

    // The one valid entry survives; the malformed siblings are dropped rather
    // than corrupting the read or throwing.
    expect(frecencyScore("c:/ok.txt", 1_700_000_000_000)).toBeGreaterThan(0);
    expect(frecencyScore("c:/bad-opens.txt")).toBe(0);
    expect(frecencyScore("c:/missing-fields.txt")).toBe(0);
  });

  test("entries that aren't an object at all (array, primitive) are discarded wholesale", () => {
    storage.setItem(STORAGE_KEY, JSON.stringify({ v: 1, entries: ["not", "a", "map"] }));

    expect(topPaths(5)).toEqual([]);
  });
});

describe("localStorage unavailable or throwing", () => {
  test("a throwing storage never propagates out of recordOpen/frecencyScore/topPaths/clearFrecency", () => {
    (globalThis as { localStorage?: Storage }).localStorage = new ThrowingStorage();

    // No `toThrow` matcher in this repo's bun:test shim — calling directly is
    // enough: an uncaught throw here fails the test on its own.
    recordOpen("C:/whatever.txt");
    expect(frecencyScore("C:/whatever.txt")).toBe(0);
    expect(topPaths(5)).toEqual([]);
    clearFrecency();
  });

  test("a missing localStorage global degrades to no-history rather than throwing", () => {
    delete (globalThis as { localStorage?: Storage }).localStorage;

    recordOpen("C:/whatever.txt");
    expect(frecencyScore("C:/whatever.txt")).toBe(0);
    expect(topPaths(5)).toEqual([]);
    clearFrecency();
  });
});

describe("sortByFrecency", () => {
  test("orders candidates by descending frecency", () => {
    const now = 1_700_000_000_000;
    recordOpen("C:/z.txt", now - 60 * DAY_MS);
    recordOpen("C:/y.txt", now);

    const items = [{ path: "C:/z.txt" }, { path: "C:/y.txt" }, { path: "C:/unknown.txt" }];
    const sorted = sortByFrecency(items, (item) => item.path, now);

    expect(sorted.map((item) => item.path)).toEqual(["C:/y.txt", "C:/z.txt", "C:/unknown.txt"]);
  });

  test("ties (equal — typically zero — score) keep their original input order", () => {
    const items = [{ path: "C:/unknown-a.txt" }, { path: "C:/unknown-b.txt" }, { path: "C:/unknown-c.txt" }];

    const sorted = sortByFrecency(items, (item) => item.path);

    expect(sorted.map((item) => item.path)).toEqual(["C:/unknown-a.txt", "C:/unknown-b.txt", "C:/unknown-c.txt"]);
  });
});
