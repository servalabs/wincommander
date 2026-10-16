import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";

const launcher = readFileSync("tools/dev.ps1", "utf8");
const server = readFileSync("tools/dev-server.ts", "utf8");
const prepared = JSON.parse(readFileSync("src-tauri/commander-free/tauri.prepared-dev.conf.json", "utf8"));

describe("desktop preparation outside the frontend deadline", () => {
  test("finishes native preparation and checks failure before invoking Tauri", () => {
    const prepare = launcher.indexOf("& $bun run tools/dev-server.ts --precleaned --prepare-only");
    const tauri = launcher.indexOf("& $tauriCli dev");
    expect(prepare).toBeGreaterThan(0);
    expect(prepare).toBeLessThan(tauri);
    expect(launcher.slice(prepare, tauri)).toContain("if ($LASTEXITCODE -ne 0)");
    expect(launcher).toContain("--config src-tauri/commander-free/tauri.prepared-dev.conf.json");
  });

  test("prepared config changes only the frontend hook and leaves security/defaults intact", () => {
    expect(prepared).toEqual({ build: { beforeDevCommand: "bun run tools/dev-server.ts --vite-only" } });
  });

  test("Vite-only starts without native cleanup or re-running slow preparation", () => {
    const start = server.indexOf("if (VITE_ONLY)");
    const inspect = server.indexOf("const inspection =");
    expect(start).toBeGreaterThan(0);
    expect(start).toBeLessThan(inspect);
    expect(server.slice(start, inspect)).toMatch(/startVite\(\);\s*return;/);
    expect(server.slice(start, inspect)).not.toContain("freeVitePort");
  });

  test("mounted or uncertain Vault state cannot be treated as completed native preparation", () => {
    const refusal = server.indexOf("if (PREPARE_ONLY && frontendOnly)");
    const preparation = server.indexOf("const steps:");
    expect(refusal).toBeGreaterThan(0);
    expect(refusal).toBeLessThan(preparation);
    expect(server.slice(refusal, preparation)).toContain("throw new Error");
    expect(server.indexOf("if (PREPARE_ONLY) {")).toBeGreaterThan(server.indexOf("await syncDevelopmentService("));
  });

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
