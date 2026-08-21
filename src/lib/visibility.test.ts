import { describe, expect, test } from "bun:test";
import { isVisible, type VisibilityCtx } from "./visibility";
import { getDependencyIds, getPersonaForSetupChoices } from "./personaMigration";
import { getToggleVisibility, type ToggleDef } from "../types/toggles";

const baseCtx: VisibilityCtx = {
  density: "guided",
  profiles: new Set(),
  dependencies: new Set(),
};

function makeToggle(overrides: Partial<ToggleDef>): ToggleDef {
  return {
    tier: "free",
    needsAdmin: false,
    irreversible: false,
    reducesSecurity: false,
    defenderFlagged: false,
    id: "sample",
    label: "Sample",
    description: "Sample toggle",
    domain: "privacy",
    settingsPath: "ideal.sample",
    currentPath: "current.sample",
    enableCmd: "Enable-Sample",
    disableCmd: "Disable-Sample",
    minExperience: "simple",
    ...overrides,
  };
}

describe("visibility resolver", () => {
  test("guided safeguards users can see safeguards cards but not expert cards", () => {
    const ctx: VisibilityCtx = {
      ...baseCtx,
      profiles: new Set(["safeguards"]),
    };

    expect(isVisible({ capability: ["safeguards"] }, ctx)).toBe(true);
    expect(isVisible({ minDensity: "expert" }, ctx)).toBe(false);
  });

  test("legacy standard safeguards toggles stay expert unless safeguards capability is selected", () => {
    const toggle = makeToggle({
      minExperience: "standard",
      capability: ["safeguards"],
    });

    expect(getToggleVisibility(toggle, new Set(["privacy"]))).toMatchObject({
      minDensity: "expert",
      capability: ["safeguards"],
    });

    expect(getToggleVisibility(toggle, new Set(["safeguards"]))).toMatchObject({
      minDensity: "guided",
      capability: ["safeguards"],
    });
  });

  test("setup choices persist density and capability bundles for the resolver", () => {
    expect(getPersonaForSetupChoices("standard", true, {
      privacy: true,
      network: true,
      mesh: false,
    })).toEqual({
      density: "guided",
      capabilities: ["privacy", "network", "safeguards"],
    });

    expect(getPersonaForSetupChoices("advanced", false, {
      privacy: false,
      network: false,
      mesh: false,
    })).toEqual({
      density: "expert",
      capabilities: [],
    });
  });

  test("dependency ids ignore null backend fields instead of crashing visibility", () => {
    const ids = getDependencyIds([
      {
        id: "Everything",
        name: null,
        panelId: null,
        installed: true,
      },
      {
        id: null,
        name: "WizTree",
        panelId: "dashboard",
        installed: true,
      },
      {
        id: "Skipped",
        name: "Not installed",
        panelId: "apps",
        installed: false,
      },
    ] as any);

    expect(Array.from(ids).sort()).toEqual(["dashboard", "everything", "wiztree"]);
  });
});
