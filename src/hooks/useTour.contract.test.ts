import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import {
  getActiveTourStepId,
  getLockdownChoicePendingEnabled,
  isTourActive,
  settleLockdownChoiceIfSaved,
  setActiveTourStepId,
  setLockdownChoicePendingEnabled,
  setTourActive,
} from "../lib/tourActive";

const source = readFileSync("src/hooks/useTour.ts", "utf8").replace(/\r\n/g, "\n");
const appSource = readFileSync("src/App.tsx", "utf8").replace(/\r\n/g, "\n");

describe("tour first-panel navigation", () => {
  test("publishes active tour state before dispatching a cross-panel navigation", () => {
    const activation = source.indexOf("setTourActive(true);");
    const navigation = source.indexOf('window.dispatchEvent(new CustomEvent("navigate-panel"');
    expect(activation).toBeGreaterThan(-1);
    expect(navigation).toBeGreaterThan(activation);
  });

  test("keeps disabled modules visible for the duration of the tour", () => {
    expect(appSource).toContain("const tourActive = useTourActive();");
    expect(appSource).toContain("!isModuleEnabled(appSettings?.app?.modules, moduleId) && !tourActive) {\n      setActivePanel('dashboard');");
    expect(appSource).toContain("!isModuleEnabled(appSettings?.app?.modules, moduleId) && !isTourActive()) {\n      return;");
    expect(appSource).toContain("[activePanel, appSettings?.app?.modules, tourActive]");
  });

  test("publishes activation synchronously for an immediate navigation event", () => {
    setTourActive(false);
    setTourActive(true);
    expect(isTourActive()).toBe(true);
    setTourActive(false);
  });

  test("publishes and clears the current step for temporary tour anchors", () => {
    setActiveTourStepId(null);
    setActiveTourStepId("dashboard-tour-lockdown-choice");
    expect(getActiveTourStepId()).toBe("dashboard-tour-lockdown-choice");
    setActiveTourStepId(null);
    expect(getActiveTourStepId()).toBe(null);
  });

  test("keeps a pending Lockdown preview after the tour closes until settings confirm the write", () => {
    setLockdownChoicePendingEnabled(null);
    setActiveTourStepId("dashboard-tour-lockdown-choice");
    setLockdownChoicePendingEnabled(true);
    expect(getLockdownChoicePendingEnabled()).toBe(true);
    setActiveTourStepId(null);
    expect(getLockdownChoicePendingEnabled()).toBe(true);
    settleLockdownChoiceIfSaved(false);
    expect(getLockdownChoicePendingEnabled()).toBe(true);
    settleLockdownChoiceIfSaved(true);
    expect(getLockdownChoicePendingEnabled()).toBe(null);
  });
});
