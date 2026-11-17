// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, test } from "bun:test";
import { FileIconCache, MAX_FILE_ICON_CACHE_BYTES } from "./fileIconCache";

describe("FileIconCache bounds", () => {
  test("entry count stays bounded and the oldest entry is evicted first", () => {
    const cache = new FileIconCache({ maxEntries: 3, now: () => 0 });
    for (let i = 0; i < 10; i++) cache.set(`p${i}`, "data");
    expect(cache.size).toBe(3);
    expect(cache.get("p0")).toBeUndefined();
    expect(cache.get("p6")).toBeUndefined();
    expect(cache.get("p7")).toBe("data");
    expect(cache.get("p9")).toBe("data");
  });

  test("a read refreshes recency so the least recently used entry goes", () => {
    const cache = new FileIconCache({ maxEntries: 2, now: () => 0 });
    cache.set("a", "1");
    cache.set("b", "2");
    cache.get("a");
    cache.set("c", "3");
    expect(cache.get("b")).toBeUndefined();
    expect(cache.get("a")).toBe("1");
  });

  test("byte budget is enforced and oversized icons are not cached", () => {
    const cache = new FileIconCache({ maxBytes: 100, now: () => 0 });
    cache.set("a", "x".repeat(20));
    cache.set("b", "x".repeat(20));
    cache.set("c", "x".repeat(20));
    expect(cache.byteSize <= 100).toBe(true);
    expect(cache.get("a")).toBeUndefined();
    cache.set("huge", "x".repeat(200));
    expect(cache.get("huge")).toBeUndefined();
  });

  test("default budget is 8 MiB", () => {
    expect(MAX_FILE_ICON_CACHE_BYTES).toBe(8 * 1024 * 1024);
  });

  test("null (negative) results are distinguishable from a miss", () => {
    const cache = new FileIconCache({ now: () => 0 });
    cache.set("none", null);
    expect(cache.get("none")).toBeNull();
    expect(cache.get("other")).toBeUndefined();
  });
});
