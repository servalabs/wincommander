import type { Finding, Rule } from "../../types/generated/fleet";

export function resolveDetectorRule(preset: Rule, localRules: Rule[], fleetRules: Rule[]): Rule {
  return fleetRules.find((rule) => rule.id === preset.id)
    ?? localRules.find((rule) => rule.id === preset.id) ?? preset;
}

export function localDetectorOverride(current: Rule, patch: Partial<Rule>): Rule {
  return { ...current, ...patch, locked: false, revision: current.revision + 1,
    actions: current.actions.filter((action) => action !== "report_fleet" && action !== "alert_admin") };
}

export function describeFinding(finding: Finding): string {
  const reasons = finding.reasons.map((reason) => reason.kind.replaceAll("_", " ")).join(", ") || "pattern matched";
  return `${finding.category.replaceAll("_", " ")} (${finding.count}): ${reasons}`;
}
