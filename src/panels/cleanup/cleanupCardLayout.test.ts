import { describe, expect, test } from "bun:test";
import { CLEAN_CARDS_PER_GRID_SLOT, packCleanCards } from "./cleanupCardLayout";

describe("packCleanCards", () => {
  test("preserves source order while fitting four clean cards per grid slot", () => {
    expect(packCleanCards(["a", "b", "c", "d", "e", "f", "g", "h", "i"])).toEqual([
      ["a", "b", "c", "d"],
      ["e", "f", "g", "h"],
      ["i"],
    ]);
  });

  test("uses a four-card footprint", () => {
    expect(CLEAN_CARDS_PER_GRID_SLOT).toBe(4);
  });
});
