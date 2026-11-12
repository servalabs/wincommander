import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { CheckboxControl } from "@/components/ui/bp";
import type { Rule, Severity } from "../../types/generated/fleet";
import { LOCAL_RULE_SEVERITIES, editableMatcherValue, setLocalAction, updateEditableMatcher } from "./LocalClipboardRules";

export default function LocalClipboardRuleForm({ rule, error, onChange, onSave, onCancel }: {
  rule: Rule;
  error: string | null;
  onChange: (rule: Rule) => void;
  onSave: () => void | Promise<void>;
  onCancel: () => void;
}) {
  const matcherKind = rule.matcher.kind === "regex" ? "regex" : "phrase";
  const matchValue = editableMatcherValue(rule.matcher);
  const updateMatcher = (kind: "phrase" | "regex", value: string) =>
    onChange({ ...rule, matcher: updateEditableMatcher(rule.matcher, kind, value) });
  return (
    <div className="flex flex-col gap-3 rounded border border-[var(--color-accent)]/40 bg-[var(--color-bg-secondary)] p-3">
      <Input aria-label="Local rule name" value={rule.name} onChange={(event) => onChange({ ...rule, name: event.currentTarget.value })} placeholder="Rule name" />
      <div className="grid grid-cols-1 gap-2 sm:grid-cols-[8rem_1fr]">
        <select aria-label="Match type" value={matcherKind} onChange={(event) => updateMatcher(event.currentTarget.value as "phrase" | "regex", matchValue)} className="rounded border border-[var(--color-border)] bg-[var(--color-bg-primary)] px-2 text-xs">
          <option value="phrase">Phrase</option>
          <option value="regex">Regular expression</option>
        </select>
        <Input aria-label="Text to match" value={matchValue} onChange={(event) => updateMatcher(matcherKind, event.currentTarget.value)} placeholder={matcherKind === "phrase" ? "Text to detect" : "Regular expression"} />
      </div>
      <div className="grid grid-cols-2 gap-2">
        <select aria-label="Severity" value={rule.severity} onChange={(event) => onChange({ ...rule, severity: event.currentTarget.value as Severity })} className="rounded border border-[var(--color-border)] bg-[var(--color-bg-primary)] px-2 py-1.5 text-xs">
          {LOCAL_RULE_SEVERITIES.map((severity) => <option key={severity} value={severity}>{severity}</option>)}
        </select>
        <Input aria-label="Cooldown seconds" type="number" min={0} max={86400} value={String(rule.cooldownSeconds)} onChange={(event) => onChange({ ...rule, cooldownSeconds: Number(event.currentTarget.value) })} />
      </div>
      <div className="flex flex-wrap gap-2 text-[10px] text-[var(--shield-text-subtle)]">
        <LocalActionOption
          checked={rule.actions.includes("notify_user")}
          label="Notify me"
          onCheckedChange={(checked) => onChange(setLocalAction(rule, "notify_user", checked))}
        />
        <LocalActionOption
          checked={rule.actions.includes("mask_matches")}
          label="Mask matches (Pro)"
          onCheckedChange={(checked) => onChange(setLocalAction(rule, "mask_matches", checked))}
        />
        <LocalActionOption
          checked={rule.actions.includes("clear_clipboard")}
          label="Clear clipboard (Pro)"
          onCheckedChange={(checked) => onChange(setLocalAction(rule, "clear_clipboard", checked))}
        />
        <LocalActionOption
          checked={rule.actions.includes("quarantine_clipboard")}
          label="Replace with a warning (Pro)"
          onCheckedChange={(checked) => onChange(setLocalAction(rule, "quarantine_clipboard", checked))}
        />
      </div>
      {error && <p role="alert" className="text-[10px] text-[var(--color-danger)]">{error}</p>}
      <div className="flex justify-end gap-2">
        <Button size="sm" onClick={onCancel}>Cancel</Button>
        <Button size="sm" variant="primary" onClick={onSave}>Save local rule</Button>
      </div>
    </div>
  );
}

function LocalActionOption({
  checked,
  label,
  onCheckedChange,
}: {
  checked: boolean;
  label: string;
  onCheckedChange: (checked: boolean) => void;
}) {
  return (
    <div
      className="flex cursor-pointer items-center gap-2 rounded border border-[var(--shield-inner-border)] bg-[var(--color-bg-primary)] px-2 py-1.5 transition-colors hover:border-[var(--color-accent)]/45"
      onClick={() => onCheckedChange(!checked)}
    >
      <CheckboxControl
        checked={checked}
        onChange={(event) => onCheckedChange(event.currentTarget.checked)}
        onClick={(event) => event.stopPropagation()}
        ariaLabel={label}
      />
      <span className="font-medium">{label}</span>
    </div>
  );
}
