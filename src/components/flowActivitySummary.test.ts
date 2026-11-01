import { describe, expect, test } from "bun:test";
import {
  clipboardActivitySummary,
  flowDecisionSummary,
  flowExecutionSummary,
  flowExecutionOutcome,
  flowExecutionActivity,
  flowNotifySummary,
} from "./flowActivitySummary";

describe("Flow DevTools activity summaries", () => {
  test("keeps clipboard patterns, decoy paths, SSIDs, and action payloads out of summaries", () => {
    const clipboard = clipboardActivitySummary({ severity: "danger", pattern: "seed phrase" });
    const execution = flowExecutionSummary({
      flowId: "FLOW-7QK4",
      completed: true,
      totalDurationMs: 42,
      actions: [{ command: "copy C:\\Users\\Alice\\secret.txt" }],
    });

    expect(clipboard).toBe("DETECT clipboard policy event (severity danger)");
    expect(execution).toBe("FLOW execution completed (flow FLOW-7QK4) after 42ms");
    expect(`${clipboard} ${execution}`).not.toContain("seed phrase");
    expect(`${clipboard} ${execution}`).not.toContain("secret.txt");
  });

  test("uses only opaque safe references and closed decision/severity values", () => {
    expect(flowDecisionSummary({ ruleId: "private rule name", reason: "admit" }))
      .toBe("FLOW decision admitted");
    expect(flowNotifySummary({ severity: "danger", message: "SSID: office-private" }))
      .toBe("FLOW notification emitted (severity danger)");
  });

  test("reports failed, completed and unknown activity without inventing a result", () => {
    expect(flowExecutionOutcome({ completed: true, failedActionCount: 0 })).toBe("completed");
    expect(flowExecutionOutcome({ completed: false, failedActionCount: 2 })).toBe("incomplete");
    expect(flowExecutionOutcome({})).toBe("unknown");
    expect(flowExecutionOutcome({ completed: null, failedActionCount: 0 })).toBe("unknown");
    expect(flowExecutionOutcome({ completed: true, dryRun: true })).toBe("simulated");
    expect(flowExecutionOutcome({ completed: true, failedActionCount: 1 })).toBe("incomplete");
    expect(flowExecutionActivity({ completed: false, failedActionCount: 2 })).toContain("2 action(s) failed");
    expect(flowExecutionActivity({ completed: true })).toContain("reported completed");
    expect(flowExecutionActivity({ actionCount: 2 })).toContain("outcome unknown");
    expect(flowExecutionActivity({ completed: true, dryRun: true })).toContain("Simulation ended");
    expect(flowExecutionActivity({ completed: false, dryRun: true })).toContain("notifications and delays may have run");
    expect(flowExecutionSummary({ completed: false })).toBe("FLOW execution incomplete or unconfirmed");
    expect(flowExecutionSummary({})).toBe("FLOW execution outcome unknown");
  });
});
