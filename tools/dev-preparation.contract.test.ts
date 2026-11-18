import { describe, expect, test } from "bun:test";
import { spawnSync } from "node:child_process";

describe("desktop preparation outside the frontend deadline", () => {
  test("conflicting preparation modes fail before running any build or service command", () => {
    for (const mode of ["--vite-only", "--frontend-only"]) {
      const result = spawnSync(process.execPath, ["run", "tools/dev-server.ts", "--prepare-only", mode], {
        encoding: "utf8", timeout: 10_000,
      });
      expect(result.status).toBe(1);
      expect(result.stderr).toContain("--prepare-only cannot be combined");
      expect(result.stdout).not.toContain("building Pro");
    }
  });
});
