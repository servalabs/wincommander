import { describe, expect, test } from "bun:test";
import { GUIDE_TOPICS } from "./index";
import { PANEL_MANIFESTS } from "../../types/panels";
import { resolveTourSteps } from "../../lib/tour";

describe("guide content SSOT", () => {
  test("topic ids are unique", () => {
    const ids = GUIDE_TOPICS.map((t) => t.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  test("every panelId references a real panel", () => {
    const real = new Set(PANEL_MANIFESTS.map((m) => m.id));
    for (const t of GUIDE_TOPICS) {
      if (t.panelId) expect(real.has(t.panelId)).toBe(true);
    }
  });

  test("tour stop orders are unique within each tour", () => {
    const seen = new Map<string, Set<number>>();
    for (const t of GUIDE_TOPICS) {
      for (const m of t.tour?.tours ?? []) {
        const orders = seen.get(m.id) ?? new Set<number>();
        expect(orders.has(m.order)).toBe(false);
        orders.add(m.order);
        seen.set(m.id, orders);
      }
    }
  });

  test("the welcome tour has stops and expert sees fewer than guided", () => {
    const guided = resolveTourSteps(GUIDE_TOPICS, "welcome", "guided");
    const expert = resolveTourSteps(GUIDE_TOPICS, "welcome", "expert");
    expect(guided.length > 0).toBe(true);
    expect(expert.length < guided.length).toBe(true);
  });

  test("skips Lockdown when the action is not visible and separates DNS from Hosts guidance", () => {
    const dashboard = resolveTourSteps(GUIDE_TOPICS, "tour-dashboard", "guided", { lockdownVisible: false });
    expect(dashboard.some((step) => step.topicId === "dashboard-tour-lockdown")).toBe(false);
    const ids = dashboard.map((step) => step.topicId);
    expect(ids.includes("network-tour-dns-firewall")).toBe(true);
    expect(ids.includes("network-tour-hosts-blocklists")).toBe(true);
  });
});
