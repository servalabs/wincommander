import { describe, expect, test } from "bun:test";
import { DEEP_DFIR_CATEGORIES, STANDARD_CATEGORIES } from "./cleanupCategories";
import {
  DEFAULT_BULK_CLEAR_EXCLUDES,
  MANUAL_ONLY_SCHEDULE_IDS,
  getCleanupAutoScheduleConfirmation,
  getCleanupManualConfirmation,
  isAutomaticCleanupExcluded,
  isCleanupCommandSuccessful,
  isDefaultExcludedCleanup,
  isManualOnlyCleanupSchedule,
  requestCleanupManualAuthorization,
} from "./cleanupManualPolicy";

const cards = [...STANDARD_CATEGORIES, ...DEEP_DFIR_CATEGORIES];

describe("explicit per-card cleanup consent", () => {
  test("default exclusions and additional user choices stay out of Auto-set", () => {
    expect(DEFAULT_BULK_CLEAR_EXCLUDES.length).toBe(10);
    const eligible = cards.filter(card => card.schedulable && !isAutomaticCleanupExcluded(card.id, new Set()));
    expect(eligible.length).toBe(37);
    for (const id of DEFAULT_BULK_CLEAR_EXCLUDES) {
      expect(isAutomaticCleanupExcluded(id.toUpperCase(), new Set())).toBe(true);
      expect(eligible.some(card => card.id === id)).toBe(false);
    }
    expect(isAutomaticCleanupExcluded("dnsCache", new Set(["DNSCACHE"]))).toBe(true);
    expect(isAutomaticCleanupExcluded("dnsCache", new Set())).toBe(false);
    expect(MANUAL_ONLY_SCHEDULE_IDS.length).toBe(33);
    expect(cards.filter(card => card.schedulable && isManualOnlyCleanupSchedule(card.id)).length).toBe(33);
    for (const id of MANUAL_ONLY_SCHEDULE_IDS) {
      expect(isManualOnlyCleanupSchedule(id.toUpperCase())).toBe(true);
      expect(eligible.some(card => card.id === id)).toBe(false);
    }
  });

  test("cancelling a default-excluded manual action does not authorize it", async () => {
    const browser = cards.find(card => card.id === "browserFootprints")!;
    let confirmations = 0;
    const cancelled = await requestCleanupManualAuthorization(browser, "schedule", async options => {
      confirmations++;
      expect(options.description).toContain("every 60 minutes");
      expect(options.description).toContain("until you turn it off");
      return false;
    }, 60);
    expect(cancelled).toBe(false);
    expect(confirmations).toBe(1);
    expect(await requestCleanupManualAuthorization(browser, "clear", async () => true)).toBe(true);
  });

  test("a newly schedulable card requires explicit individual confirmation", async () => {
    const card = cards.find(item => item.id === "shadowCopies")!;
    let prompted = false;
    const accepted = await requestCleanupManualAuthorization(card, "schedule", async options => {
      prompted = true;
      expect(options.description).toContain("every 60 minutes");
      expect(options.description).toContain("excluded from automatic setup");
      return false;
    }, 60);
    expect(prompted).toBe(true);
    expect(accepted).toBe(false);
  });

  test("ordinary cards do not receive the default-exclusion override warning", async () => {
    const dns = cards.find(card => card.id === "dnsCache")!;
    let confirmations = 0;
    const allowed = await requestCleanupManualAuthorization(dns, "schedule", async () => {
      confirmations++;
      return false;
    }, 60);
    expect(allowed).toBe(true);
    expect(confirmations).toBe(0);
  });

  test("every default exclusion explains its individual destructive effect", () => {
    for (const card of cards.filter(card => isDefaultExcludedCleanup(card.id))) {
      const warning = getCleanupManualConfirmation(card, "clear");
      expect(warning.description).toContain("excluded from bulk cleanup by default");
      expect(warning.description).toContain("cannot be undone");
      expect(warning.confirmLabel).toBe("Clear this card");
    }
  });

  test("protected schedule warnings disclose saved passwords and SSH host keys", () => {
    expect(getCleanupManualConfirmation(cards.find(card => card.id === "browserFootprints")!, "schedule", 60).description).toContain("saved browser passwords");
    expect(getCleanupManualConfirmation(cards.find(card => card.id === "sshState")!, "schedule", 60).description).toContain("known_hosts");
  });

  test("Auto-set names high-impact cards and explains repeated deletion", () => {
    const eligible = cards.filter(card => card.schedulable && !isAutomaticCleanupExcluded(card.id, new Set()));
    const warning = getCleanupAutoScheduleConfirmation(eligible);
    expect(warning.description).toContain("37 cards");
    expect(warning.description).toContain("repeatedly and permanently delete");
    expect(warning.description).toContain("Recycle Bin Records");
    expect(warning.description).not.toContain("Shadow Copies");
    expect(warning.description).not.toContain("Game Captures");
    expect(warning.description).toContain("additional exclusions will be skipped");
  });

  test("a transport success never hides a failed or partial cleanup receipt", () => {
    for (const data of [{ ok: false }, { error: true }, { partial: true }, { complete: false }, { status: "failed" }, { status: "partial" }, { stdout: "fail: residual=1" }]) {
      expect(isCleanupCommandSuccessful({ success: true, data })).toBe(false);
    }
    expect(isCleanupCommandSuccessful({ success: false })).toBe(false);
    expect(isCleanupCommandSuccessful({ success: true, data: { ok: true } })).toBe(true);
  });
});

test("each protected schedule confirmation identifies the selected account and honors cancellation", async () => {
  const target = "Other User (S-1-5-21-100-200-300-1001)";
  for (const id of DEFAULT_BULK_CLEAR_EXCLUDES) {
    const card = cards.find(item => item.id === id)!;
    expect(card).toBeDefined();
    let calls = 0;
    const allowed = await requestCleanupManualAuthorization(card, "schedule", async options => {
      calls++;
      expect(options.description).toContain(target);
      expect(options.description).toContain("every 60 minutes");
      return false;
    }, 60, target);
    expect(calls).toBe(1);
    expect(allowed).toBe(false);
  }
});
