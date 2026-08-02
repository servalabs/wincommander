import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const source = readFileSync("src/panels/dev/index.tsx", "utf8");

describe("dev panel loading state", () => {
  test("does not render a blank panel while checking the build gate", () => {
    expect(source).not.toContain("if (isDev === null) return null");
    expect(source).toContain("Checking whether Dev Tools are available…");
    expect(source).toContain('role="status" aria-busy="true"');
  });
});
