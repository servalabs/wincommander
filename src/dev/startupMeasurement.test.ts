import { describe, expect, test } from "bun:test";
import { summarizeStartupSamples, validateStartupSample } from "./startupMeasurement";

describe("startup measurement", () => {
  test("rejects empty measurements and unknown scenarios before summarizing", () => {
    expect(validateStartupSample({ scenario: "warm", elapsedMs: {} }).length).toBeGreaterThan(0);
    expect(validateStartupSample({ scenario: "unrecognized", elapsedMs: { process_start: 0 } }).length).toBeGreaterThan(0);
    let error: unknown;
    try { summarizeStartupSamples([{ scenario: "warm", elapsedMs: {} }]); } catch (caught) { error = caught; }
    expect(error instanceof Error).toBe(true);
  });

  test("rejects malformed JSON shapes without throwing from validation", () => {
    for (const sample of [null, [], "sample", {}, { scenario: "warm", elapsedMs: null }, { scenario: "warm", elapsedMs: [] }]) {
      expect(validateStartupSample(sample).length).toBeGreaterThan(0);
    }
  });

  test("rejects invalid timings, unknown phases, and conflicting protection outcomes", () => {
    for (const value of [-1, NaN, Infinity, "12", null]) {
      expect(validateStartupSample({ scenario: "warm", elapsedMs: { process_start: value } }).length).toBeGreaterThan(0);
    }
    expect(validateStartupSample({ scenario: "warm", elapsedMs: { process_start: 0, typo: 10 } }).length).toBeGreaterThan(0);
    expect(validateStartupSample({ scenario: "warm", elapsedMs: { protection_required_ready: 10, protection_failed: 10 } }).length).toBeGreaterThan(0);
  });

  test("rejects reordered or unsafe timing samples", () => {
    expect(validateStartupSample({ scenario: "warm", elapsedMs: { process_start: 20, native_setup_entered: 10 } }))
      .toEqual(["native_setup_entered: out of order"]);
  });

  test("reports deterministic p50 and nearest-rank p95 without a performance claim", () => {
    const samples = [10, 20, 30, 40, 50].map((elapsed) => ({ scenario: "warm" as const, elapsedMs: { process_start: 0, dashboard_interactive: elapsed } }));
    expect(summarizeStartupSamples(samples)).toEqual([{
      scenario: "warm", samples: 5, phases: {
        process_start: { samples: 5, p50: 0, p95: 0, max: 0 },
        dashboard_interactive: { samples: 5, p50: 30, p95: 50, max: 50 },
      },
    }]);
  });

  test("reports each phase's own sample count in partial diagnostics", () => {
    const summaries = summarizeStartupSamples([
      { scenario: "warm", elapsedMs: { process_start: 0, dashboard_interactive: 25 } },
      { scenario: "warm", elapsedMs: { process_start: 0 } },
    ]);
    expect(summaries[0].samples).toBe(2);
    expect(summaries[0].phases.process_start?.samples).toBe(2);
    expect(summaries[0].phases.dashboard_interactive?.samples).toBe(1);
  });
});
