import { describe, expect, test } from "bun:test";
import { summarizeStartupSamples, validateStartupSample } from "./startupMeasurement";

describe("startup measurement", () => {
  test("rejects reordered or unsafe timing samples", () => {
    expect(validateStartupSample({ scenario: "warm", elapsedMs: { process_start: 20, native_setup_entered: 10 } }))
      .toEqual(["native_setup_entered: out of order"]);
  });

  test("reports deterministic p50 and nearest-rank p95 without a performance claim", () => {
    const samples = [10, 20, 30, 40, 50].map((elapsed) => ({ scenario: "warm" as const, elapsedMs: { process_start: 0, dashboard_interactive: elapsed } }));
    expect(summarizeStartupSamples(samples)).toEqual([{
      scenario: "warm", samples: 5, phases: {
        process_start: { p50: 0, p95: 0, max: 0 },
        dashboard_interactive: { p50: 30, p95: 50, max: 50 },
      },
    }]);
  });
});
