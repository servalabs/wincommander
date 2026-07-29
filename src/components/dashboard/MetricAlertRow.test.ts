import { describe, expect, test } from "bun:test";
import { getMetricAlertBellAction } from "./MetricAlertRow";

describe("getMetricAlertBellAction", () => {
  test("keeps a disabled Network Traffic alert disabled and requests feedback", () => {
    expect(getMetricAlertBellAction(false, true)).toBe("shake");
  });

  test("retains the normal toggle behaviour for enabled and non-network alerts", () => {
    expect(getMetricAlertBellAction(true, true)).toBe("toggle");
    expect(getMetricAlertBellAction(false, false)).toBe("toggle");
  });
});
