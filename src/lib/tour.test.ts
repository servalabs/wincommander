import { describe, expect, test } from "bun:test";
import { resolveTourSteps, tourIdForPanel } from "./tour";
import type { GuideTopic } from "../content/guide/types";

const TOPICS: GuideTopic[] = [
  {
    id: "a",
    title: "A",
    summary: "first",
    body: "",
    tour: { anchor: "#a", tours: [{ id: "welcome", order: 30 }] },
  },
  {
    id: "b",
    title: "B",
    summary: "second",
    body: "",
    tour: { anchor: "#b", placement: "right", tours: [{ id: "welcome", order: 10 }] },
  },
  {
    id: "c",
    title: "C",
    summary: "guided-only",
    body: "",
    tour: { anchor: "#c", tours: [{ id: "welcome", order: 20, densities: ["guided"] }] },
  },
  {
    id: "d",
    title: "D",
    summary: "other tour",
    body: "",
    tour: { anchor: "#d", tours: [{ id: "deep", order: 10 }] },
  },
  { id: "e", title: "E", summary: "no tour", body: "" },
  {
    id: "f",
    title: "F",
    summary: "dashboard tour stop",
    body: "",
    tour: { anchor: "#f", tours: [{ id: "tour-dashboard", order: 10 }] },
  },
];

describe("resolveTourSteps", () => {
  test("returns only stops for the named tour, sorted by order", () => {
    const steps = resolveTourSteps(TOPICS, "welcome");
    expect(steps.map((s) => s.topicId)).toEqual(["b", "c", "a"]);
  });

  test("drops density-restricted stops for the wrong density", () => {
    const steps = resolveTourSteps(TOPICS, "welcome", "expert");
    expect(steps.map((s) => s.topicId)).toEqual(["b", "a"]);
  });

  test("keeps density-restricted stops for the matching density", () => {
    const steps = resolveTourSteps(TOPICS, "welcome", "guided");
    expect(steps.map((s) => s.topicId)).toEqual(["b", "c", "a"]);
  });

  test("defaults placement to auto and carries anchor/navigateTo", () => {
    const [first] = resolveTourSteps(TOPICS, "welcome");
    expect(first.anchor).toBe("#b");
    expect(first.placement).toBe("right");
    const last = resolveTourSteps(TOPICS, "welcome").at(-1)!;
    expect(last.placement).toBe("auto");
  });

  test("returns [] for an unknown tour", () => {
    expect(resolveTourSteps(TOPICS, "nope")).toEqual([]);
  });

  test("omits controls unavailable in the current session", () => {
    const conditionalTopics: GuideTopic[] = [
      {
        id: "scrub",
        title: "Scrub",
        summary: "",
        body: "",
        tour: {
          anchor: "#scrub",
          showWhen: (ctx) => ctx.scrubMetadataVisible !== false,
          tours: [{ id: "tour-dashboard", order: 10 }],
        },
      },
      {
        id: "lockdown",
        title: "Lockdown",
        summary: "",
        body: "",
        tour: {
          anchor: "#lockdown",
          showWhen: (ctx) => ctx.lockdownVisible !== false,
          tours: [{ id: "tour-dashboard", order: 20 }],
        },
      },
    ];

    expect(resolveTourSteps(conditionalTopics, "tour-dashboard", undefined, {
      scrubMetadataVisible: false,
      lockdownVisible: false,
    })).toEqual([]);
  });
});

describe("tourIdForPanel", () => {
  test("returns the panel's own tour id when one exists in the registry", () => {
    expect(tourIdForPanel(TOPICS, "dashboard")).toBe("tour-dashboard");
  });

  test("falls back to welcome when the panel has no tour of its own", () => {
    expect(tourIdForPanel(TOPICS, "privacy")).toBe("welcome");
  });
});
