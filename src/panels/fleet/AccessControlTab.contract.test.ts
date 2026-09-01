// src/panels/fleet/AccessControlTab.contract.test.ts
//
// AccessControlTab is a React component with no DOM test harness in this repo
// (see useChipSearch.test.ts / useManagedPolicy.test.ts — every hook/component
// test here exercises an exported pure function or, for wiring that cannot be
// pulled out as a pure function, the component source directly). The two
// things below fall in that second bucket: the functional-setState fix and
// the Save-groups call graph are structural, not computable from inputs.

import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const source = readFileSync("src/panels/fleet/AccessControlTab.tsx", "utf8");

describe("AccessControlTab avoids clobbering discovered users with a stale onChange snapshot", () => {
  test("addGroup, updateGroup, and deleteGroup all use the functional onChange(current => ...) form", () => {
    const addGroup = source.slice(source.indexOf("const addGroup"), source.indexOf("const updateGroup"));
    const updateGroup = source.slice(source.indexOf("const updateGroup"), source.indexOf("const toggleUser"));
    const deleteGroup = source.slice(source.indexOf("const deleteGroup"), source.indexOf("const visibleGroups"));

    for (const [, body] of [["addGroup", addGroup], ["updateGroup", updateGroup], ["deleteGroup", deleteGroup]] as const) {
      expect(body).toMatch(/onChange\(current => /);
      expect(body).not.toContain("...directory,");
    }
  });

  test("discoverUsers already uses the functional form (the pattern these callers now match)", () => {
    const discoverUsers = source.slice(source.indexOf("const discoverUsers"), source.indexOf("useEffect(() => {\n    if (discoveredOnce"));
    expect(discoverUsers).toMatch(/onChange\(current => /);
  });
});

describe("Save groups reconciles real Windows groups instead of only writing localStorage", () => {
  test("save() persists locally, then reconciles through the Vault access service", () => {
    const save = source.slice(source.indexOf("const save = ()"), source.lastIndexOf("return ("));
    expect(save).toContain("onSave()");
    expect(save).toContain("reconcileGroups()");
  });

  test("reconcileGroups sends SIDs built by the shared plan builder and reports honest per-group outcomes", () => {
    expect(source).toContain("buildAccessGroupReconcilePlan(directory)");
    expect(source).toContain("reconcileAccessGroups(requests)");
    expect(source).toContain("summarizeReconcileResults(results)");
    expect(source).toContain('outcome.intent === "danger"');
  });

  test("a rejected call is classified instead of surfacing a raw or silent failure", () => {
    expect(source).toContain("describeReconcileFailure(cause)");
  });

  test("does not invent a manual account row (that regression is Access Control's other rule)", () => {
    expect(source).not.toContain("addManualUser");
  });
});
