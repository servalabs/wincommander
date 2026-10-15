import { expect, test } from "bun:test";
import { readDevelopmentVaultState } from "./dev-service-startup";

test("development preparation distinguishes mounted state from an empty verified journal", () => {
  expect(readDevelopmentVaultState('{"state":"mounted","mountedCount":1}', 0)).toEqual({ state: "mounted", mountedCount: 1 });
  expect(readDevelopmentVaultState('{"state":"free","mountedCount":0}', 0)).toEqual({ state: "free", mountedCount: 0 });
});

test("failed, malformed and inconsistent observations preserve native processes", () => {
  for (const [output, code] of [["", 0], ['{"state":"free","mountedCount":1}', 0],
    ['{"state":"mounted","mountedCount":0}', 0], ['{"state":"free","mountedCount":0}', 1],
    ['{"state":"free","mountedCount":0}', null], ["unexpected diagnostic", 0]] as const) {
    expect(readDevelopmentVaultState(output, code).state).toBe("unknown");
  }
});
