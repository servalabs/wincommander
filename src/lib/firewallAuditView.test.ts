import { describe, expect, it } from "bun:test";
import {
  backupFolderOf,
  classifyFirewallError,
  firewallConfirmCopy,
  firewallRemoveConsequences,
} from "./firewallAuditCopy";
import {
  buildRemediationMessage,
  filterFirewallRules,
  readFirewallRule,
  sortFirewallRules,
  summarizeFirewallAudit,
} from "./firewallAuditView";
import type { FirewallRemediation, FirewallRule } from "../hooks/useBackend";

const rule = (over: Partial<FirewallRule>): FirewallRule => ({
  id: over.name ?? "r1",
  name: "Some App Inbound",
  enabled: true,
  action: "Allow",
  program: "C:\\Program Files\\App\\app.exe",
  signed: null,
  ...over,
});

const remediation = (over: Partial<FirewallRemediation>): FirewallRemediation => ({
  changed: 0,
  cancelled: false,
  errors: [],
  backupPath: null,
  ...over,
});

describe("readFirewallRule", () => {
  it("calls an enabled allow with no program the broadest kind of rule", () => {
    const reading = readFirewallRule(rule({ program: "" }));
    expect(reading.concern).toBe("allow-all");
    expect(reading.tone).toBe("warning");
  });

  it("treats an enabled allow scoped to one program as neutral", () => {
    expect(readFirewallRule(rule({})).concern).toBe("allow-app");
  });

  it("warns that a disabled block rule is protection switched off", () => {
    const reading = readFirewallRule(rule({ enabled: false, action: "Block" }));
    expect(reading.concern).toBe("block-off");
    expect(reading.tone).toBe("warning");
    expect(reading.advice).toContain("Enable it");
  });

  it("calls a disabled allow inactive clutter", () => {
    const reading = readFirewallRule(rule({ enabled: false, action: "Allow" }));
    expect(reading.concern).toBe("inactive");
    expect(reading.advice).toContain("Safe to remove");
  });

  it("is case-insensitive about the action string netsh emits", () => {
    expect(readFirewallRule(rule({ enabled: false, action: "BLOCK" })).concern).toBe("block-off");
  });
});

describe("sortFirewallRules", () => {
  it("puts the rules that matter first when sorting by state", () => {
    const sorted = sortFirewallRules([
      rule({ name: "inactive", enabled: false, action: "Allow" }),
      rule({ name: "narrow", enabled: true, action: "Allow" }),
      rule({ name: "blockoff", enabled: false, action: "Block" }),
      rule({ name: "broad", enabled: true, action: "Allow", program: "" }),
    ]);
    expect(sorted.map((r) => r.name)).toEqual(["broad", "blockoff", "narrow", "inactive"]);
  });

  it("does not mutate the input", () => {
    const input = [rule({ name: "b" }), rule({ name: "a" })];
    sortFirewallRules(input, "name");
    expect(input[0].name).toBe("b");
  });

  it("sorts by name alphabetically", () => {
    const sorted = sortFirewallRules([rule({ name: "Zeta" }), rule({ name: "Alpha" })], "name");
    expect(sorted[0].name).toBe("Alpha");
  });
});

describe("filterFirewallRules", () => {
  it("matches on program path as well as rule name", () => {
    const rules = [rule({ name: "Alpha", program: "C:\\x\\zoom.exe" }), rule({ name: "Beta", program: "" })];
    expect(filterFirewallRules(rules, "zoom").map((r) => r.name)).toEqual(["Alpha"]);
  });

  it("returns the original list for an empty query", () => {
    const rules = [rule({})];
    expect(filterFirewallRules(rules, "   ")).toBe(rules);
  });
});

describe("summarizeFirewallAudit", () => {
  it("says the audit is incomplete when it was cancelled", () => {
    expect(summarizeFirewallAudit([], true).intent).toBe("warning");
  });

  it("explains a clean result rather than showing an empty table", () => {
    const summary = summarizeFirewallAudit([], false);
    expect(summary.intent).toBe("success");
    expect(summary.detail).toContain("never listed");
  });

  it("counts broad allows and switched-off blocks in the headline", () => {
    const summary = summarizeFirewallAudit(
      [
        rule({ name: "broad", program: "" }),
        rule({ name: "off", enabled: false, action: "Block" }),
        rule({ name: "narrow" }),
      ],
      false,
    );
    expect(summary.intent).toBe("warning");
    expect(summary.headline).toContain("2 of 3");
    expect(summary.detail).toContain("disabling is reversible");
  });

  it("calls a list of only narrow allows housekeeping, not security", () => {
    const summary = summarizeFirewallAudit([rule({ name: "narrow" })], false);
    expect(summary.intent).toBe("success");
    expect(summary.detail).toContain("housekeeping");
  });
});

describe("buildRemediationMessage", () => {
  // Regression: the previous inline version keyed the tone off audit.error,
  // which is null on a remediation failure, so a total failure rendered green.
  it("reports a total failure as danger, never success", () => {
    const outcome = buildRemediationMessage("remove", remediation({ changed: 0, errors: ["a: denied"] }));
    expect(outcome.intent).toBe("danger");
    expect(outcome.text).toContain("Nothing changed");
  });

  it("reports a partial failure as a warning", () => {
    const outcome = buildRemediationMessage("disable", remediation({ changed: 2, errors: ["a: denied"] }));
    expect(outcome.intent).toBe("warning");
    expect(outcome.text).toContain("2 rules disabled");
  });

  it("reports a cancelled run as a warning naming what already changed", () => {
    const outcome = buildRemediationMessage("remove", remediation({ changed: 3, cancelled: true }));
    expect(outcome.intent).toBe("warning");
    expect(outcome.text).toContain("Stopped early");
    expect(outcome.text).toContain("3 rules removed");
  });

  it("flags a zero-change zero-error run as stale rather than success", () => {
    expect(buildRemediationMessage("enable", remediation({})).intent).toBe("warning");
  });

  it("reports a clean run as success with the right verb", () => {
    const outcome = buildRemediationMessage("enable", remediation({ changed: 1 }));
    expect(outcome.intent).toBe("success");
    expect(outcome.text).toBe("1 rule enabled.");
  });
});

describe("classifyFirewallError", () => {
  it("maps the locale failure to a next step", () => {
    const advice = classifyFirewallError("firewall rule output is unsupported on this Windows locale");
    expect(advice.hint).toContain("display language");
  });

  it("maps an access-denied netsh failure to an elevation hint", () => {
    expect(classifyFirewallError("Access is denied.").needsElevation).toBe(true);
  });

  it("maps the expired cache to a rescan", () => {
    expect(
      classifyFirewallError("firewall audit expired; scan again before remediation").title,
    ).toContain("too old");
  });

  it("keeps an unknown error visible", () => {
    expect(classifyFirewallError("weird netsh noise").hint).toBe("weird netsh noise");
  });
});

describe("firewallRemoveConsequences", () => {
  it("names the rules and truncates a long selection", () => {
    const bullets = firewallRemoveConsequences(["a", "b", "c", "d", "e", "f", "g", "h"], 3);
    expect(bullets[0]).toContain("8 firewall rules");
    expect(bullets).toContain("• …and 5 more");
  });

  it("always mentions the automatic backup and how to restore it", () => {
    const bullets = firewallRemoveConsequences(["a"]);
    expect(bullets.some((b) => b.includes("netsh advfirewall import"))).toBe(true);
  });
});

describe("firewallConfirmCopy", () => {
  it("gives the irreversible action an enforced pause and the reversible ones none", () => {
    expect(firewallConfirmCopy("remove", ["a"]).countdownSeconds).toBe(3);
    expect(firewallConfirmCopy("disable", ["a"]).countdownSeconds).toBe(0);
    expect(firewallConfirmCopy("enable", ["a"]).countdownSeconds).toBe(0);
  });

  it("names the count in the title and pluralizes it", () => {
    expect(firewallConfirmCopy("remove", ["a"]).title).toBe("Remove 1 firewall rule?");
    expect(firewallConfirmCopy("disable", ["a", "b"]).title).toBe("Disable 2 firewall rules?");
  });

  it("says removal is permanent and the reversible actions are not", () => {
    expect(firewallConfirmCopy("remove", ["a"]).intro).toContain("for good");
    expect(firewallConfirmCopy("disable", ["a"]).intro).toContain("reversible");
  });

  it("truncates a long selection in the reversible path too", () => {
    const copy = firewallConfirmCopy("enable", ["a", "b", "c"], 2);
    expect(copy.consequences).toContain("• …and 1 more");
  });

  it("always mentions the automatic backup", () => {
    for (const action of ["enable", "disable", "remove"] as const) {
      const copy = firewallConfirmCopy(action, ["a"]);
      expect(copy.consequences.some((c) => c.toLowerCase().includes("backup"))).toBe(true);
    }
  });
});

describe("backupFolderOf", () => {
  it("returns the containing folder of the exported .wfw", () => {
    expect(backupFolderOf("C:\\Users\\a\\AppData\\wc\\firewall-audit-1.wfw")).toBe(
      "C:\\Users\\a\\AppData\\wc",
    );
  });

  it("returns the input unchanged when there is no separator", () => {
    expect(backupFolderOf("firewall.wfw")).toBe("firewall.wfw");
  });
});
