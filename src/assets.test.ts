import { describe, expect, test } from "bun:test";
import { applyProductAliases } from "./assets/products";

describe("applyProductAliases", () => {

  test("does not inject undefined aliases when no fallback asset exists", () => {
    const aliased = applyProductAliases({});
    const keys = Object.keys(aliased);

    expect(keys).not.toContain("contingency/decoy-hub.png");
    expect(keys).not.toContain("private-phone/hero.png");
    expect(keys).not.toContain("wincommander/hero.png");
  });
});
