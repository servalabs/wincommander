import { afterEach, describe, expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { linkSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { completeStartupFixture } from "../src/dev/startupBenchmark.fixture";

const root = fileURLToPath(new URL("../", import.meta.url));
const folders: string[] = [];

afterEach(() => {
  for (const folder of folders.splice(0)) {
    if (dirname(folder) !== resolve(tmpdir())) throw new Error("Unexpected fixture cleanup directory");
    rmSync(folder, { recursive: true, force: true });
  }
});

function invoke(input: unknown, flags: string[] = [], output = false) {
  const folder = mkdtempSync(join(tmpdir(), "wc-startup-benchmark-"));
  folders.push(folder);
  const source = join(folder, "samples.json");
  const target = join(folder, "report.json");
  writeFileSync(source, JSON.stringify(input));
  const result = spawnSync(process.execPath, [join(root, "tools/startup-benchmark.ts"), source, ...(output ? [target] : []), ...flags], { cwd: root, encoding: "utf8" });
  expect(result.error).toBeUndefined();
  return { ...result, source, target };
}

describe("startup benchmark CLI", () => {
  test("returns nonzero for empty or unknown measurements and explains their exclusion", () => {
    const result = invoke([{ scenario: "warm", elapsedMs: {} }, { scenario: "unknown", elapsedMs: { process_start: 0 } }]);
    expect(result.status).toBe(1);
    expect(JSON.parse(result.stdout)).toMatchObject({ dataComplete: false, acceptedSamples: 0, rejectedSamples: 2, summaries: [] });
  });

  test("partial diagnostics are allowed only without the completeness requirement", () => {
    const partial = [{ scenario: "warm", elapsedMs: { process_start: 0 } }];
    expect(invoke(partial).status).toBe(0);
    const result = invoke(partial, ["--require-complete"]);
    expect(result.status).toBe(1);
    expect(JSON.parse(result.stdout)).toMatchObject({ dataComplete: false, incompleteSamples: 1 });
  });

  test("writes a complete report with source metadata and remaining external gates", () => {
    const input = completeStartupFixture();
    const result = invoke(input, ["--require-complete"], true);
    expect(result.status).toBe(0);
    expect(result.stderr).toBe("");
    const report = JSON.parse(readFileSync(result.target, "utf8"));
    expect(report).toMatchObject({ reportOnly: true, dataComplete: true, completeSamples: 1, metadata: input.metadata });
    expect(report.generatedAt).toBeString();
    expect(report.externalGates).toContain("packaged Windows");
  });

  test("does not qualify complete timings without build metadata", () => {
    const result = invoke(completeStartupFixture().samples, ["--require-complete"]);
    expect(result.status).toBe(1);
    expect(JSON.parse(result.stdout)).toMatchObject({ dataComplete: false, completeSamples: 0, incompleteSamples: 1 });
  });

  test("fails cleanly for malformed input shapes, empty input, and unknown options", () => {
    for (const input of [null, [], { schemaVersion: 1, samples: "bad" }]) expect(invoke(input).status).toBe(1);
    const invalidOption = invoke(completeStartupFixture(), ["--complete"]);
    expect(invalidOption.status).toBe(1);
    expect(invalidOption.stderr).toContain("Usage:");
  });

  test("refuses to overwrite raw samples and does not echo malformed JSON contents", () => {
    const result = invoke(completeStartupFixture());
    const original = readFileSync(result.source, "utf8");
    const overwrite = spawnSync(process.execPath, [join(root, "tools/startup-benchmark.ts"), result.source, result.source], { encoding: "utf8" });
    expect(overwrite.status).toBe(1);
    expect(readFileSync(result.source, "utf8")).toBe(original);
    writeFileSync(result.source, '{ "private-fixture-marker": INVALID }');
    const malformed = spawnSync(process.execPath, [join(root, "tools/startup-benchmark.ts"), result.source], { encoding: "utf8" });
    expect(malformed.status).toBe(1);
    expect(malformed.stderr).toBe("Invalid startup sample JSON.\n");
  });

  test("preserves raw samples through hard-link aliases and preserves existing reports", () => {
    const result = invoke(completeStartupFixture());
    const original = readFileSync(result.source, "utf8");
    linkSync(result.source, result.target);
    const alias = spawnSync(process.execPath, [join(root, "tools/startup-benchmark.ts"), result.source, result.target], { encoding: "utf8" });
    expect(alias.status).toBe(1);
    expect(readFileSync(result.source, "utf8")).toBe(original);
    expect(readFileSync(result.target, "utf8")).toBe(original);
    const existing = invoke(completeStartupFixture(), [], true);
    const report = readFileSync(existing.target, "utf8");
    const repeat = spawnSync(process.execPath, [join(root, "tools/startup-benchmark.ts"), existing.source, existing.target], { encoding: "utf8" });
    expect(repeat.status).toBe(1);
    expect(readFileSync(existing.target, "utf8")).toBe(report);
  });
});
