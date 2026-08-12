import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const source = readFileSync("src/panels/network/index.tsx", "utf8");

describe("network populated-state interaction contracts", () => {
  test("brand blocklists and DNS categories are native pressed buttons", () => {
    expect(source).toContain("aria-label={`${isApplied ? 'Disable' : 'Enable'} ${name.replace(/-/g, ' ')} blocklist`}");
    expect(source).toContain("aria-label={`${active ? 'Disable' : 'Enable'} ${label} DNS category`}");
    expect(source).toContain("disabled={isAtLimit}");
  });
});
