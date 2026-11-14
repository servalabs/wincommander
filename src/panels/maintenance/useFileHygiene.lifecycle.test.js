import { expect, test } from "bun:test";
import { fileURLToPath } from "node:url";

test("removal and navigation retain safe report state", () => {
  const fixture = fileURLToPath(new URL("./useFileHygiene.lifecycle.fixture.js", import.meta.url));
  const result = Bun.spawnSync([process.execPath, "run", fixture], { stdout: "pipe", stderr: "pipe" });
  expect(result.stderr.toString()).toBe("");
  expect(result.exitCode).toBe(0);
});
