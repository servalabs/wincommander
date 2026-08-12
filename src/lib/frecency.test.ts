import { beforeEach, describe, expect, test } from "bun:test";
import { clearFrecency, frecencyScore, normalizeKey, recordOpen, sortByFrecency, topPaths } from "./frecency";

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

  // Defect 1 regression: eviction must be decoupled from the decaying
  // ranking score, or a heavy-use idle file gets permanently forgotten.
  test("MEASURED: a 200-open file idle 100 days scores 86.44, below a brand-new single open's 100", () => {
    const now = 1_700_000_000_000;
    recordOpen("C:/heavy-use.exe", now - 100 * DAY_MS);
    for (let i = 1; i < 200; i++) recordOpen("C:/heavy-use.exe", now - 100 * DAY_MS);
    recordOpen("C:/brand-new.txt", now);

    // RANKING legitimately prefers recency (intentional) — the bug was
    // letting this same score decide EVICTION too; see the flood test below.
    // (No toBeCloseTo in this repo's bun:test shim — round explicitly.)
    const heavyUseScore = Math.round(frecencyScore("C:/heavy-use.exe", now) * 100) / 100;
    expect(heavyUseScore).toBe(86.44);
    expect(frecencyScore("C:/brand-new.txt", now)).toBe(100);
    expect(frecencyScore("C:/brand-new.txt", now)).toBeGreaterThan(frecencyScore("C:/heavy-use.exe", now));
  });
});

describe("normalizeKey", () => {
  test("a trailing separator collapses to the same entry as no trailing separator", () => {
    expect(normalizeKey("D:/Projects/Notes.txt/")).toBe(normalizeKey("D:/Projects/Notes.txt"));
    expect(normalizeKey("D:\\Projects\\Notes.txt\\")).toBe(normalizeKey("D:/Projects/Notes.txt"));
  });

  test("doubled internal separators collapse to the same entry", () => {
    expect(normalizeKey("D:\\\\Projects\\\\Notes.txt")).toBe(normalizeKey("D:/Projects/Notes.txt"));
    expect(normalizeKey("D://Projects//Notes.txt")).toBe(normalizeKey("D:/Projects/Notes.txt"));
  });

  test("a \\\\?\\ extended-length prefix collapses to the plain drive path", () => {
    expect(normalizeKey("\\\\?\\D:\\Projects\\Notes.txt")).toBe(normalizeKey("D:/Projects/Notes.txt"));
  });

  test("a \\\\?\\UNC\\ extended-length prefix collapses to the plain UNC path", () => {
    expect(normalizeKey("\\\\?\\UNC\\server\\share\\file.txt")).toBe(normalizeKey("\\\\server\\share\\file.txt"));
  });

  test("a doubled separator inside a UNC path still collapses, without losing the leading //", () => {
    expect(normalizeKey("\\\\server\\\\share\\file.txt")).toBe(normalizeKey("\\\\server\\share\\file.txt"));
  });

  test("recordOpen + frecencyScore actually match across these variants (not just the raw key fn)", () => {
    const now = 1_700_000_000_000;
    recordOpen("D:\\Projects\\Notes.txt", now);

    expect(frecencyScore("D:/Projects/Notes.txt/", now)).toBeGreaterThan(0);
    expect(frecencyScore("D:\\\\Projects\\\\Notes.txt", now)).toBeGreaterThan(0);
    expect(frecencyScore("\\\\?\\D:\\Projects\\Notes.txt", now)).toBeGreaterThan(0);
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

  // Defect 1, the actual reported failure: before the fix, enforceCap sorted
  // by the decaying ranking score, so a heavy-use file idle past 90 days
  // (score 86.44) scored below every one-off opened-today entry (score 100)
  // — it was the FIRST thing evicted by a same-day flood, not the last.
  test("a heavy-use idle file survives a same-day flood of 3000 one-off opens", () => {
    const now = 1_700_000_000_000;
    const idleSince = now - 100 * DAY_MS; // past the 90-day STALE_WEIGHT floor

    // 200 lifetime opens, all idle — the vulnerable shape per the task (any
    // file under ~512 lifetime opens, idle past 90 days).
    for (let i = 0; i < 200; i++) recordOpen("C:/heavy-use.exe", idleSince);
    // Same-day flood of distinct one-off opens, past the 500-entry cap alone.
    for (let i = 0; i < 3000; i++) recordOpen(`C:/flood/one-off-${i}.txt`, now);

    // History must survive — not silently, permanently erased by entries
    // that are each individually worth less.
    expect(frecencyScore("C:/heavy-use.exe", now)).toBeGreaterThan(0);

    const raw = storage.getItem(STORAGE_KEY);
    const parsed = JSON.parse(raw!) as { entries: Record<string, unknown> };
    expect(parsed.entries["c:/heavy-use.exe"]).toBeTruthy();
    expect(Object.keys(parsed.entries)).toHaveLength(500);
  });
});

describe("recordOpen performance (Defect 3 — hot path, runs on every file open)", () => {
  test("recordOpen stays fast with the store already at full 500-entry capacity", () => {
    const now = 1_700_000_000_000;
    // Fill the store to the cap first so every timed call below pays the
    // full readStore/enforceCap/writeStore cost against 500 entries.
    for (let i = 0; i < 500; i++) recordOpen(`C:/warm/file${i}.txt`, now - i * DAY_MS);

    const iterations = 1000;
    const start = performance.now();
    for (let i = 0; i < iterations; i++) {
      recordOpen(`C:/warm/file${i % 500}.txt`, now + i);
    }
    const elapsedMs = performance.now() - start;
    const perCallMs = elapsedMs / iterations;

    // Reported (measured on this machine, this run): see verification output.
    // Threshold is generous on purpose — this asserts "not accidentally
    // quadratic / not blocking a render", not a tight perf budget.
    // (This repo's bun:test shim has no toBeLessThan — express perCallMs < 5
    // as "5 is greater than perCallMs" using the matcher that does exist.)
    expect(5).toBeGreaterThan(perCallMs);
    // Surfaces the real measured number in test output per the task's
    // "time it" requirement — not asserted beyond the threshold above
    // because absolute timing is machine-dependent.
    console.log(`recordOpen @ 500-entry cap: ${perCallMs.toFixed(4)}ms/call over ${iterations} iterations`);
  });

  test("the parse cache invalidates when another WebView changes localStorage", () => {
    const now = 1_700_000_000_000;
    recordOpen("C:/main-window.txt", now);

    storage.setItem(STORAGE_KEY, JSON.stringify({
      v: 1,
      entries: {
        "c:/overlay-window.txt": { path: "C:/overlay-window.txt", opens: 2, lastOpened: now },
      },
    }));

    expect(frecencyScore("C:/main-window.txt", now)).toBe(0);
    expect(frecencyScore("C:/overlay-window.txt", now)).toBeGreaterThan(0);
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
