import { describe, expect, test } from "bun:test";
import {
  clipboardActivitySummary,
  flowDecisionSummary,
  flowExecutionSummary,
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
});
