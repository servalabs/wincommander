import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const launcher = readFileSync("tools/dev.ps1", "utf8");
const server = readFileSync("tools/dev-server.ts", "utf8");

describe("public-checkout development bootstrap", () => {
  test("starts Free development when the private Pro workspace is unavailable", () => {
    expect(launcher).toContain("function Test-ProWorkspaceAvailable");
    expect(launcher).toContain("WINCOMMANDER_DEV_FREE_ONLY");
    expect(launcher).toContain("Pro workspace not found; starting the Free-only development app.");
  });

  test("keeps the existing Pro build only when Free-only mode is not selected", () => {
    expect(server).toContain('process.env.WINCOMMANDER_DEV_FREE_ONLY === "1"');
    expect(server).toContain("if (!FREE_ONLY)");
  });
});
