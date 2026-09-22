import { describe, expect, test } from "bun:test";
import { createStartupBenchmarkReport } from "./startupBenchmark";
import { completeStartupFixture } from "./startupBenchmark.fixture";

describe("startup benchmark qualification", () => {
  test("requires non-empty sample input and a recognized envelope version", () => {
    for (const input of [null, [], {}, { schemaVersion: 2, samples: [{}] }, { schemaVersion: 1, samples: "bad" }]) {
      let error: unknown;
      try { createStartupBenchmarkReport(input); } catch (caught) { error = caught; }
      expect(error instanceof Error).toBe(true);
    }
  });

  test("rejects empty and unknown samples without counting them as measurements", () => {
    const report = createStartupBenchmarkReport([
      { scenario: "warm", elapsedMs: {} },
      { scenario: "unknown", elapsedMs: { process_start: 0 } },
    ]);
    expect(report).toMatchObject({ dataComplete: false, samples: 2, acceptedSamples: 0, rejectedSamples: 2, completeSamples: 0, summaries: [] });
    expect(report.sampleIssues.map((issue) => issue.status)).toEqual(["rejected", "rejected"]);
  });

  test("keeps legacy partial diagnostics explicitly incomplete", () => {
    const report = createStartupBenchmarkReport([{ scenario: "warm", elapsedMs: { process_start: 0 } }]);
    expect(report).toMatchObject({ reportOnly: true, dataComplete: false, acceptedSamples: 1, incompleteSamples: 1, completeSamples: 0 });
    expect(report.metadataIssues.length).toBeGreaterThan(0);
    expect(report.sampleIssues[0].issues).toContain("protection: missing outcome timing");
  });

  test("qualifies complete supplied samples while listing unmeasured scenarios and external gates", () => {
    const report = createStartupBenchmarkReport(completeStartupFixture());
    expect(report).toMatchObject({ reportOnly: true, dataComplete: true, acceptedSamples: 1, completeSamples: 1, incompleteSamples: 0, rejectedSamples: 0, metadataIssues: [], sampleIssues: [] });
    expect(report.unmeasuredScenarios).toEqual(["cold", "first-install", "offline", "downloads-50k"]);
    expect(report.externalGates).toContain("packaged Windows");
  });

  test("a missing required phase prevents qualification even with valid metadata", () => {
    const input = completeStartupFixture();
    const { background_idle: _idle, ...partial } = input.samples[0].elapsedMs;
    const report = createStartupBenchmarkReport({ ...input, samples: [{ scenario: "warm", elapsedMs: partial }] });
    expect(report.dataComplete).toBe(false);
    expect(report.sampleIssues[0].issues).toContain("background_idle: missing timing");
  });

  test("rejects incomplete or inconsistent build and environment provenance", () => {
    for (const patch of [
      { freeRevision: "short" }, { proArtifactHash: null }, { machineId: " " },
      { windowsVersion: "" }, { webviewVersion: null }, { capturedAt: "2026-09-22" },
      { capturedAt: "2026-02-30T00:00:00.000Z" }, { protectionRequired: "true" },
      { downloadsEntries: -1 }, { downloadsEntries: 2.5 }, { unknown: "not retained" },
    ]) {
      const input = completeStartupFixture();
      const report = createStartupBenchmarkReport({ ...input, metadata: { ...input.metadata, ...patch } });
      expect(report.dataComplete).toBe(false);
      expect(report.metadata).toBeNull();
      expect(report.metadataIssues.length).toBeGreaterThan(0);
    }
  });

  test("supports an explicitly absent Pro artifact and non-required protection", () => {
    const input = completeStartupFixture();
    const { protection_required_ready, ...timings } = input.samples[0].elapsedMs;
    const report = createStartupBenchmarkReport({
      ...input,
      metadata: { ...input.metadata, proRevision: null, proArtifactHash: null, protectionRequired: false },
      samples: [{ scenario: "warm", elapsedMs: { ...timings, protection_not_required: protection_required_ready } }],
    });
    expect(report.dataComplete).toBe(true);
  });

  test("failed or contradictory protection can never qualify successful startup data", () => {
    const input = completeStartupFixture();
    const { protection_required_ready, ...timings } = input.samples[0].elapsedMs;
    for (const outcome of ["protection_failed", "protection_not_required"]) {
      const report = createStartupBenchmarkReport({ ...input, samples: [{ scenario: "warm", elapsedMs: { ...timings, [outcome]: protection_required_ready } }] });
      expect(report.dataComplete).toBe(false);
      expect(report.completeSamples).toBe(0);
    }
  });

  test("does not silently discard a bad sample beside a complete one", () => {
    const input = completeStartupFixture();
    const report = createStartupBenchmarkReport({ ...input, samples: [...input.samples, { scenario: "typo", elapsedMs: { process_start: 0 } }] });
    expect(report).toMatchObject({ dataComplete: false, completeSamples: 1, acceptedSamples: 1, rejectedSamples: 1 });
  });

  test("requires protection readiness within the measured startup interval", () => {
    const input = completeStartupFixture();
    input.samples[0].elapsedMs.protection_required_ready = 81;
    expect(createStartupBenchmarkReport(input).dataComplete).toBe(false);
    input.samples[0].elapsedMs.protection_required_ready = 80;
    expect(createStartupBenchmarkReport(input).dataComplete).toBe(true);
  });

  test("checks the 50000-entry scenario against the recorded fixture size", () => {
    const input = completeStartupFixture();
    input.samples[0].scenario = "downloads-50k";
    expect(createStartupBenchmarkReport(input).dataComplete).toBe(true);
    input.metadata.downloadsEntries = 20;
    expect(createStartupBenchmarkReport(input).dataComplete).toBe(false);
  });
});
