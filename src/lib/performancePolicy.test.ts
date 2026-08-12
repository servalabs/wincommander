import { describe, expect, test } from "bun:test";
import { STARTUP_STAGGER_PLAN } from "./performancePolicy";

describe("startup performance policy", () => {
  test("stages expensive startup work after dashboard startup", () => {
    expect(STARTUP_STAGGER_PLAN.map((step) => step.stage)).toEqual([
      "dependencies",
      "mesh",
      "inventory",
    ]);

    const delays = STARTUP_STAGGER_PLAN.map((step) => step.delayMs);
    expect(delays[0] > 0).toBe(true);
    expect(delays[1] > delays[0]).toBe(true);
    expect(delays[2] > delays[1]).toBe(true);
    expect(STARTUP_STAGGER_PLAN.find((step) => step.stage === "inventory")?.runWhenIdle).toBe(true);
  });
});
