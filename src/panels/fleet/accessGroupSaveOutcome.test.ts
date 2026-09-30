import { describe, expect, test } from "bun:test";
import { accessGroupSaveOutcome } from "./accessGroupSaveOutcome";
import type { VaultSaveAccessDirectoryResponse } from "./accessControlTypes";

function receipt(): VaultSaveAccessDirectoryResponse {
  return { directory: { schema_version: 1, users: [], groups: [{ id: "group-1", name: "Team", local_group: "WC_Team", member_sids: ["S-1-5-21-101"] }] }, results: [{ local_group: "WC_Team", state: "unchanged", error: null }] };
}

describe("confirmed machine-wide group save feedback", () => {
  test("created, updated and unchanged Windows groups all confirm success", () => {
    for (const state of ["created", "updated", "unchanged"] as const) {
      const saved = receipt(); saved.results[0].state = state;
      expect(accessGroupSaveOutcome(saved).intent).toBe("success");
      expect(accessGroupSaveOutcome(saved).message).toContain("on this PC");
      if (state === "unchanged") expect(accessGroupSaveOutcome(saved).message).toContain("Saved group settings confirmed");
      else expect(accessGroupSaveOutcome(saved).message).toContain('Group "Team" saved');
    }
  });
  test("failed, duplicate or unrecognized readback never confirms success", () => {
    const failed = receipt(); failed.results[0].state = "failed";
    const duplicate = receipt(); duplicate.results.push({ ...duplicate.results[0] });
    const unknown = receipt(); unknown.results[0].state = "pending" as never;
    for (const saved of [failed, duplicate, unknown]) expect(accessGroupSaveOutcome(saved).intent).toBe("danger");
  });
  test("unchanged outsider groups need no reconciliation result", () => {
    const saved = receipt(); saved.results = [];
    expect(accessGroupSaveOutcome(saved).intent).toBe("success");
    expect(accessGroupSaveOutcome(saved).message).not.toContain("Team");
    expect(accessGroupSaveOutcome(saved).message).toContain("Saved group settings confirmed");
  });
  test("only explicitly updated groups are named, not unchanged outsider groups", () => {
    const saved = receipt(); saved.results[0].state = "updated";
    saved.directory.groups.push({ id: "other", name: "Outsider group", local_group: "WC_Other", member_sids: [] });
    expect(accessGroupSaveOutcome(saved).message).toContain('Group "Team" saved');
    expect(accessGroupSaveOutcome(saved).message).not.toContain("Outsider group");
  });
  test("same durable directory gives the same receipt outcome across administrator accounts", () => {
    const firstAdmin = receipt(); const otherAdmin = structuredClone(firstAdmin);
    expect(accessGroupSaveOutcome(otherAdmin)).toEqual(accessGroupSaveOutcome(firstAdmin));
    expect(accessGroupSaveOutcome(otherAdmin).message).toContain("authorized Windows administrators");
  });
  test("confirmed empty directory is a valid deletion result", () => {
    const saved = receipt(); saved.directory.groups = []; saved.results = [];
    expect(accessGroupSaveOutcome(saved).intent).toBe("success");
  });
});
