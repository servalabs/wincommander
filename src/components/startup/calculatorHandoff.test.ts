import { expect, test } from "bun:test";
import { advanceCalculatorHandoff } from "./calculatorHandoff";

test("four consecutive CE presses request one handoff", () => {
  let count = 0;
  for (let press = 1; press <= 4; press++) {
    const result = advanceCalculatorHandoff(count, true);
    expect(result.shouldOpen).toBe(press === 4);
    count = result.count;
  }
  expect(count).toBe(0);
  expect(advanceCalculatorHandoff(count, true).shouldOpen).toBe(false);
});

test("another calculator action resets the CE sequence", () => {
  expect(advanceCalculatorHandoff(3, false)).toEqual({ count: 0, shouldOpen: false });
  expect(advanceCalculatorHandoff(0, true)).toEqual({ count: 1, shouldOpen: false });
});
