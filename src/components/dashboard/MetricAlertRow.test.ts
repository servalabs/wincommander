import { describe, expect, test } from "bun:test";
import { shouldBuzzMetricAlertInput } from "./MetricAlertRow";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("shouldBuzzMetricAlertInput", () => {
  test("requests feedback only when an off network alert's input is attempted", () => {
    expect(shouldBuzzMetricAlertInput(false, true)).toBe(true);
  });

  test("does not buzz an enabled alert or a row without input feedback", () => {
    expect(shouldBuzzMetricAlertInput(true, true)).toBe(false);
    expect(shouldBuzzMetricAlertInput(false, false)).toBe(false);
  });
});

describe("System Info Fleet reporting layout", () => {
  test("uses one Fleet switch for CPU and RAM in the CPU label row", async () => {
    const source = await Bun.file("src/components/dashboard/HardwareSpecsCard.tsx").text();

    expect(source).toContain('reportToFleetMetrics={["cpu", "ram"]}');
    expect(source).toContain('metric="ram" label="RAM" unit="%" showReportToFleet={false}');
  });
});
