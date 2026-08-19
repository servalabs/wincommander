import type { Action, MatchKind, Rule, Severity } from "../../types/generated/fleet";
import { newUuidV4 } from "../../lib/uuid";

export type ClipboardRuleSource = "local" | "fleet";
export type LocalClipboardAction = Extract<Action, "notify_user" | "clear_clipboard" | "quarantine_clipboard">;
export type EditableMatchKind = Extract<MatchKind, { kind: "phrase" | "regex" }>;

export interface SourcedClipboardRule {
  rule: Rule;
  source: ClipboardRuleSource;
}

export interface LocalRuleValidation {
  isValid: boolean;
  message: string | null;
}

const LOCAL_ACTIONS = new Set<Action>(["notify_user", "clear_clipboard", "quarantine_clipboard"]);

export function createLocalClipboardRule(existingIds: ReadonlySet<string>): Rule {
  let id = newUuidV4();
  while (existingIds.has(id)) id = newUuidV4();
  return {
    id,
    revision: 1,
    name: "New local rule",
    enabled: true,
    priority: 100,
    matcher: { kind: "phrase", params: { value: "", case_sensitive: false } },
    severity: "warn",
    actions: ["notify_user"],
    cooldownSeconds: 30,
    snoozable: true,
    locked: false,
  };
}

export function validateLocalClipboardRule(
  rule: Rule,
  otherRuleIds: ReadonlySet<string>,
): LocalRuleValidation {
  if (!rule.name.trim()) return invalid("Give the rule a name.");
  if (otherRuleIds.has(rule.id)) return invalid("That rule ID is already in use.");
  if (rule.matcher.kind !== "phrase" && rule.matcher.kind !== "regex") {
    return invalid("Local rules support phrase or regular-expression matching.");
  }
  const matchText = rule.matcher.kind === "phrase"
    ? rule.matcher.params.value
    : rule.matcher.params.pattern;
  if (!matchText.trim()) return invalid("Enter text to match.");
  if (rule.actions.length === 0) return invalid("Choose at least one local action.");
  if (rule.actions.some((action) => !LOCAL_ACTIONS.has(action))) {
    return invalid("Local rules cannot report to Fleet or alert administrators.");
  }
  if (rule.cooldownSeconds < 0 || rule.cooldownSeconds > 86_400) {
    return invalid("Cooldown must be between 0 seconds and 24 hours.");
  }
  return { isValid: true, message: null };
}

export function updateEditableMatcher(
  matcher: MatchKind,
  kind: EditableMatchKind["kind"],
  value: string,
): EditableMatchKind {
  const caseSensitive = matcher.kind === "phrase" || matcher.kind === "regex"
    ? matcher.params.case_sensitive
    : false;
  return kind === "phrase"
    ? { kind, params: { value, case_sensitive: caseSensitive } }
    : { kind, params: { pattern: value, case_sensitive: caseSensitive } };
}

export function editableMatcherValue(matcher: MatchKind): string {
  if (matcher.kind === "phrase") return matcher.params.value;
  if (matcher.kind === "regex") return matcher.params.pattern;
  return "";
}

export function setLocalAction(rule: Rule, action: LocalClipboardAction, enabled: boolean): Rule {
  const actions = rule.actions.filter((item): item is LocalClipboardAction => LOCAL_ACTIONS.has(item));
  const next = enabled ? [...new Set([...actions, action])] : actions.filter((item) => item !== action);
  return { ...rule, actions: next };
}

export const LOCAL_RULE_SEVERITIES: Severity[] = ["info", "warn", "high", "critical"];

function invalid(message: string): LocalRuleValidation {
  return { isValid: false, message };
}
