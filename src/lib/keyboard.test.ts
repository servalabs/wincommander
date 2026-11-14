import { expect, test } from "bun:test";
import { isImeComposing } from "./keyboard";

test("Enter shortcuts wait for IME composition to finish", () => {
  expect(isImeComposing({ isComposing: true, keyCode: 13 })).toBe(true);
  expect(isImeComposing({ isComposing: false, keyCode: 229 })).toBe(true);
  expect(isImeComposing({ isComposing: false, keyCode: 13 })).toBe(false);
});
