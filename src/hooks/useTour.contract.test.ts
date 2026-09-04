import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { isTourActive, setTourActive } from "../lib/tourActive";

const source = readFileSync("src/hooks/useTour.ts", "utf8");
const appSource = readFileSync("src/App.tsx", "utf8");

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
});
