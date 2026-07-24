import { describe, expect, test } from "bun:test";
import type { VisibilityCtx } from "../lib/visibility";
import { getPrimaryManifests, getSidebarManifests } from "./panels";

describe("panel IA manifest", () => {
  test("primary sidebar manifests match the current cover IA", () => {
    expect(getPrimaryManifests().map((panel) => panel.label)).toEqual([
      "Dashboard",
      "Windows Settings",
      "Privacy Settings",
      "System Cleanup",
      "Maintenance",
      "Network Control",
      "Packages & Apps",
      "Secret Settings",
      "Settings",
    ]);
  });

  test("sidebar manifest excludes retired and covert-only panels", () => {
    const labels = getSidebarManifests().map((panel) => panel.label);

    expect(labels).not.toContain("Intelligence");
    expect(labels).not.toContain(["Advanced", "Tools"].join(" "));
  });

  test("capability tier entries obey the resolver while primary cover IA stays stable", () => {
    const guided: VisibilityCtx = {
      density: "guided",
      profiles: new Set(),
      dependencies: new Set(),
    };
    const network: VisibilityCtx = {
      ...guided,
      profiles: new Set(["network"]),
    };
    const expert: VisibilityCtx = {
      ...guided,
      density: "expert",
    };

    expect(getSidebarManifests(guided).map((panel) => panel.label)).toEqual([
      "Dashboard",
      "Windows Settings",
      "Privacy Settings",
      "System Cleanup",
      "Maintenance",
      "Network Control",
      "Packages & Apps",
      "Productivity",
      "Secure Storage",
      "Flows",
      "Fleet",
      "Secret Settings",
      "Settings",
    ]);
    expect(getSidebarManifests(network).map((panel) => panel.label)).toContain("Private Network");
    // AI Advisor + Search Files are right-sidebar launchers (navTier "hidden"),
    // so neither appears on the left rail even at expert density (owner 2026-06-09).
    expect(getSidebarManifests(expert).map((panel) => panel.label)).not.toContain("AI Advisor");
    expect(getSidebarManifests(expert).map((panel) => panel.label)).not.toContain("Search Files");
  });
});
