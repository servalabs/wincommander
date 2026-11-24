import { describe, expect, test } from "bun:test";
import { getUnknownError } from "./useBackend";

describe("backend error text", () => {
  test("a string rejected by a Tauri command keeps its text without literal quotes", () => {
    expect(getUnknownError("Fleet manages Privacy Shield on this device.")).toBe("Fleet manages Privacy Shield on this device.");
  });
  test("other rejections still produce readable text", () => {
    expect(getUnknownError(new Error("boom"))).toBe("boom");
    expect(getUnknownError({ code: 3 })).toBe('{"code":3}');
  });
});