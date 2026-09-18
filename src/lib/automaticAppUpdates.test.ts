import { describe, expect, test } from "bun:test";
import {
  clearAutomaticAppUpdateAttempt,
  getAutomaticAppUpdateCandidates,
  recordAutomaticAppUpdateAttempt,
} from "./automaticAppUpdates";
import type { PendingUpdateEntry } from "../types/settings";

const update = (id: string, latestVersion = "2.0.0", inManifest = true): PendingUpdateEntry => ({
  id,
  name: id,
  installedVersion: "1.0.0",
  latestVersion,
  source: "winget",
  inManifest,
});

describe("automatic app updates", () => {
  test("skips ignored updates and optionally non-catalog apps", () => {
    const candidates = getAutomaticAppUpdateCandidates({
      pendingUpdates: [update("Git.Git"), update("Other.Tool", "2.0.0", false)],
      ignoredFindingIds: ["app-update:Git.Git"],
      attemptsById: {},
      manifestOnly: true,
    });
    expect(candidates).toEqual([]);
  });

  test("permits one retry, then stops for the same available version", () => {
    const initial = update("Git.Git");
    const first = recordAutomaticAppUpdateAttempt({}, initial);
    const second = recordAutomaticAppUpdateAttempt(first, initial);
    expect(first["Git.Git"].attempts).toBe(1);
    expect(second["Git.Git"].attempts).toBe(2);
    expect(getAutomaticAppUpdateCandidates({
      pendingUpdates: [initial], ignoredFindingIds: [], attemptsById: second, manifestOnly: false,
    })).toEqual([]);
  });

  test("allows a newer available version after an older one exhausted retries", () => {
    const attempts = { "Git.Git": { version: "2.0.0", attempts: 2 } };
    expect(getAutomaticAppUpdateCandidates({
      pendingUpdates: [update("Git.Git", "2.1.0")], ignoredFindingIds: [], attemptsById: attempts, manifestOnly: false,
    })).toHaveLength(1);
  });

  test("clears retry history after a successful update", () => {
    expect(clearAutomaticAppUpdateAttempt({ "Git.Git": { version: "2.0.0", attempts: 1 } }, "Git.Git")).toEqual({});
  });
});
