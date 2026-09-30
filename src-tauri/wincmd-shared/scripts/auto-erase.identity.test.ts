import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";

test.skipIf(process.platform !== "win32")("scheduled cleanup migration preserves owners and schedules without running wipes", () => {
  const result = spawnSync("powershell.exe", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "src-tauri/wincmd-shared/scripts/auto-erase.identity.tests.ps1"], { encoding: "utf8" });
  expect(result.stderr).toBe("");
  expect(result.status).toBe(0);
  expect(result.stdout).toContain("PASS: coded identity");
});
