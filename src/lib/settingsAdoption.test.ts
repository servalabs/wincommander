import { expect, test } from "bun:test";
import { buildAdoptionPatch, createAdoptionAttempts } from "./settingsAdoption";
import type { AppSettings } from "../types/settings";

const toggle = { settingsPath: "ideal.privacy.test", currentPath: "current.privacy.test" };
const settings = (intent: unknown, observed: unknown) => ({
  ideal: { privacy: { test: intent } }, current: { privacy: { test: observed } },
}) as unknown as AppSettings;

test("adoption retains explicit intent and requires a positive observation", () => {
  expect(buildAdoptionPatch(settings(null, true), [toggle])).toEqual({ ideal: { privacy: { test: true } } });
  for (const intent of [false, true, "Deny"]) expect(buildAdoptionPatch(settings(intent, true), [toggle])).toBeNull();
  for (const observed of [null, undefined, false, "true"]) expect(buildAdoptionPatch(settings(null, observed), [toggle])).toBeNull();
  expect(buildAdoptionPatch(settings(null, true), [{ ...toggle, capabilityKey: "test" }])).toBeNull();
});

test("automatic adoption cannot target prototype or non-ideal paths", () => {
  expect(buildAdoptionPatch(settings(null, true), [{ ...toggle, settingsPath: "app.test" }])).toBeNull();
  expect(buildAdoptionPatch(settings(null, true), [{ ...toggle, settingsPath: "ideal.__proto__.test" }])).toBeNull();
});

test("failed adoption remains retryable, bounded and never acknowledged", () => {
  const attempts = createAdoptionAttempts();
  for (let index = 0; index < 3; index++) {
    expect(attempts.begin("probe")).toBe(true);
    expect(attempts.begin("probe")).toBe(false);
    attempts.finish("probe", false);
  }
  expect(attempts.begin("probe")).toBe(false);
  expect(attempts.begin("new-probe")).toBe(true);
});

test("only a confirmed matching receipt suppresses that probe", () => {
  const attempts = createAdoptionAttempts();
  expect(attempts.begin("probe")).toBe(true);
  attempts.finish("other", true);
  expect(attempts.begin("new-probe")).toBe(false);
  attempts.finish("probe", true);
  expect(attempts.begin("probe")).toBe(false);
  expect(attempts.begin("new-probe")).toBe(true);
});
