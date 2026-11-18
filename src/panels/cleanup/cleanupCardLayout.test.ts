import { describe, expect, test } from "bun:test";
import { packCleanCards } from "./cleanupCardLayout";

describe("packCleanCards", () => {
  test("preserves source order while fitting four clean cards per grid slot", () => {
    expect(packCleanCards(["a", "b", "c", "d", "e", "f", "g", "h", "i"])).toEqual([
      ["a", "b", "c", "d"],
      ["e", "f", "g", "h"],
      ["i"],
    ]);
  });

});
