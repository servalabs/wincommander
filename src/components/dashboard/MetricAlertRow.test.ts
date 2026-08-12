import { describe, expect, test } from "bun:test";
import { shouldBuzzMetricAlertInput } from "./MetricAlertRow";

describe("shouldBuzzMetricAlertInput", () => {
  test("requests feedback only when an off network alert's input is attempted", () => {
    expect(shouldBuzzMetricAlertInput(false, true)).toBe(true);
  });

  test("does not buzz an enabled alert or a row without input feedback", () => {
    expect(shouldBuzzMetricAlertInput(true, true)).toBe(false);
    expect(shouldBuzzMetricAlertInput(false, false)).toBe(false);
  });
});
