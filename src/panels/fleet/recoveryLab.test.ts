import { expect, test } from "bun:test";
import { recoveryLabEnabled } from "./recoveryLab";

test("recovery UI requires an explicit lab build flag", () => {
  expect(recoveryLabEnabled(undefined)).toBe(false);
  expect(recoveryLabEnabled("true")).toBe(false);
  expect(recoveryLabEnabled("0")).toBe(false);
  expect(recoveryLabEnabled("1")).toBe(true);
});
