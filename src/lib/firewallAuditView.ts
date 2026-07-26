// Pure presentation + interpretation helpers for the firewall audit card.
// No React, no IPC — unit tested in firewallAuditView.test.ts.
//
// WHY: `firewall_audit_preview` returns rule name / enabled / action / program
// and nothing else. On its own a row reading "disabled · Allow · all programs"
// tells a user neither what the rule does nor whether enabling, disabling or
// removing it is the safe move. readFirewallRule answers both.
import type { FirewallRemediation, FirewallRule } from "../hooks/useBackend";
import type { SortDirection } from "./arpDiagnostics";

export type FirewallAction = "enable" | "disable" | "remove";

/** What kind of attention a candidate rule deserves. The Rust `eligible()`
 *  filter only ever surfaces enabled-allow, disabled-allow and disabled-block
 *  rules, but the fallthrough keeps this total. */
export type FirewallConcern = "allow-all" | "allow-app" | "block-off" | "inactive";

export interface FirewallRuleReading {
  concern: FirewallConcern;
  /** Short state column label. */
  label: string;
  tone: "warning" | "neutral" | "info";
  /** What this rule is doing to traffic right now. */
  meaning: string;
  /** What the user should do about it. */
  advice: string;
}

const isAllow = (rule: FirewallRule): boolean => rule.action.trim().toLowerCase() === "allow";
const isBlock = (rule: FirewallRule): boolean => rule.action.trim().toLowerCase() === "block";

export function readFirewallRule(rule: FirewallRule): FirewallRuleReading {
  const hasProgram = rule.program.trim().length > 0;
  if (rule.enabled && isAllow(rule)) {
    return hasProgram
      ? {
          concern: "allow-app",
          label: "Lets one app through",
          tone: "neutral",
          meaning: "Active — traffic is allowed for this program.",
          advice: "Keep it if you recognise the program. Disable it if you don't.",
        }
      : {
          concern: "allow-all",
          label: "Lets everything through",
          tone: "warning",
          meaning: "Active — traffic is allowed for every program, not just one.",
          advice: "The broadest kind of rule. Disable it unless you know what needs it.",
        };
  }
  if (!rule.enabled && isBlock(rule)) {
    return {
      concern: "block-off",
      label: "Block is switched off",
      tone: "warning",
      meaning: "Inactive — a rule written to block traffic is currently doing nothing.",
      advice: "Enable it to put the block back, or remove it if it is obsolete.",
    };
  }
  return {
    concern: "inactive",
    label: "Inactive",
    tone: "info",
    meaning: "Inactive — this rule has no effect on traffic while it is off.",
    advice: "Leftover clutter. Safe to remove, or enable it if you still want it.",
  };
}

export type FirewallSortKey = "name" | "state" | "action" | "program";

export function sortFirewallRules(
  rules: FirewallRule[],
  key: FirewallSortKey = "state",
  direction: SortDirection = "asc",
): FirewallRule[] {
  const sign = direction === "asc" ? 1 : -1;
  // Concern order puts the rules that actually matter at the top when sorting
  // by state: broad allows first, then switched-off blocks, then noise.
  const rank: Record<FirewallConcern, number> = {
    "allow-all": 0,
    "block-off": 1,
    "allow-app": 2,
    inactive: 3,
  };
  return [...rules].sort((a, b) => {
    let primary = 0;
    if (key === "state") primary = rank[readFirewallRule(a).concern] - rank[readFirewallRule(b).concern];
    else if (key === "action") primary = a.action.localeCompare(b.action, undefined, { sensitivity: "base" });
    else if (key === "program") primary = a.program.localeCompare(b.program, undefined, { sensitivity: "base" });
    else primary = a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
    if (primary !== 0) return primary * sign;
    return a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
  });
}

export function filterFirewallRules(rules: FirewallRule[], query: string): FirewallRule[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return rules;
  return rules.filter(
    (rule) =>
      rule.name.toLowerCase().includes(needle) || rule.program.toLowerCase().includes(needle),
  );
}

export interface FirewallSummary {
  intent: "primary" | "success" | "warning";
  headline: string;
  detail: string;
}

export function summarizeFirewallAudit(rules: FirewallRule[], cancelled: boolean): FirewallSummary {
  if (cancelled) {
    return {
      intent: "warning",
      headline: "Audit stopped early — this list is incomplete",
      detail: "You cancelled before Windows finished listing rules. Run the audit again for a full picture.",
    };
  }
  if (rules.length === 0) {
    return {
      intent: "success",
      headline: "Nothing worth reviewing",
      detail:
        "Every third-party rule is either an active block or already narrow. Windows, Microsoft, Defender and WinCommander rules are never listed here.",
    };
  }
  const readings = rules.map(readFirewallRule);
  const allowAll = readings.filter((r) => r.concern === "allow-all").length;
  const blockOff = readings.filter((r) => r.concern === "block-off").length;
  const noisy = allowAll + blockOff;
  if (noisy === 0) {
    return {
      intent: "success",
      headline: `${rules.length} rule${rules.length === 1 ? "" : "s"} you could tidy up`,
      detail:
        "None of them weaken the firewall — they are narrow allows and switched-off leftovers. Removing them is housekeeping, not security.",
    };
  }
  const parts: string[] = [];
  if (allowAll) parts.push(`${allowAll} let traffic through for every program`);
  if (blockOff) parts.push(`${blockOff} ${blockOff === 1 ? "block is" : "blocks are"} switched off`);
  return {
    intent: "warning",
    headline: `${noisy} of ${rules.length} rule${rules.length === 1 ? "" : "s"} need a decision`,
    detail: `${parts.join(" and ")}. Select the ones you don't recognise, then disable them — disabling is reversible, removing is not. A full firewall backup is exported automatically before any change.`,
  };
}

export interface FirewallOutcome {
  intent: "success" | "warning" | "danger";
  text: string;
}

const PAST_TENSE: Record<FirewallAction, string> = {
  enable: "enabled",
  disable: "disabled",
  remove: "removed",
};

/** Derives the result tone from the operation outcome.
 *  WHY this is a function and not inline: the previous inline version keyed the
 *  tone off `audit.error`, which is null on a remediation failure, so a hard
 *  failure rendered inside a green success notice. */
export function buildRemediationMessage(
  action: FirewallAction,
  result: FirewallRemediation,
): FirewallOutcome {
  const failed = result.errors.length;
  const verb = PAST_TENSE[action];
  const changed = `${result.changed} rule${result.changed === 1 ? "" : "s"} ${verb}`;

  if (result.changed === 0 && failed > 0) {
    return {
      intent: "danger",
      text: `Nothing changed — all ${failed} selected rule${failed === 1 ? "" : "s"} ${failed === 1 ? "was" : "were"} refused or failed. See the reasons below.`,
    };
  }
  if (failed > 0) {
    return {
      intent: "warning",
      text: `${changed}, but ${failed} ${failed === 1 ? "was" : "were"} refused or failed. See the reasons below.`,
    };
  }
  if (result.cancelled) {
    return { intent: "warning", text: `Stopped early — ${changed} before you cancelled.` };
  }
  if (result.changed === 0) {
    return {
      intent: "warning",
      text: "No rules changed. The selection was already stale — audit again and retry.",
    };
  }
  return { intent: "success", text: `${changed}.` };
}
