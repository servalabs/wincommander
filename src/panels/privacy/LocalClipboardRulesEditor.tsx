import { useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { Icon } from "@/components/ui/icon";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { CheckboxControl } from "@/components/ui/bp";
import type { Rule, Severity } from "../../types/generated/fleet";
import {
  LOCAL_RULE_SEVERITIES,
  createLocalClipboardRule,
  editableMatcherValue,
  setLocalAction,
  updateEditableMatcher,
  validateLocalClipboardRule,
} from "./LocalClipboardRules";

interface Props {
  localRules: Rule[];
  fleetRules: Rule[];
  onChangeLocalRules: (rules: Rule[]) => void | Promise<void>;
}

export default function LocalClipboardRulesEditor({
  localRules,
  fleetRules,
  onChangeLocalRules,
}: Props) {
  const [draft, setDraft] = useState<Rule | null>(null);
  const [error, setError] = useState<string | null>(null);
  const allIds = useMemo(
    () => new Set([...localRules, ...fleetRules].map((rule) => rule.id)),
    [fleetRules, localRules],
  );

  const startAdd = () => {
    setError(null);
    setDraft(createLocalClipboardRule(allIds));
  };

  const startEdit = (rule: Rule) => {
    setError(null);
    setDraft({ ...rule, actions: [...rule.actions], locked: false });
  };

  const saveDraft = async () => {
    if (!draft) return;
    const otherIds = new Set(
      [...localRules, ...fleetRules]
        .filter((rule) => rule.id !== draft.id)
        .map((rule) => rule.id),
    );
    const validation = validateLocalClipboardRule(draft, otherIds);
    if (!validation.isValid) {
      setError(validation.message);
      return;
    }
    const existed = localRules.some((rule) => rule.id === draft.id);
    const saved = { ...draft, revision: existed ? draft.revision + 1 : 1, locked: false };
    try {
      await onChangeLocalRules(
        existed
          ? localRules.map((rule) => rule.id === saved.id ? saved : rule)
          : [...localRules, saved],
      );
      setDraft(null);
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  };

  const removeRule = async (id: string) => {
    await onChangeLocalRules(localRules.filter((rule) => rule.id !== id));
    if (draft?.id === id) setDraft(null);
  };

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center justify-between gap-3">
        <div>
          <div className="text-[10px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)]">
            Custom rules
          </div>
          <p className="mt-1 text-[10px] text-[var(--shield-text-muted)]">
            Local rules stay on this device. Fleet rules are managed by your organization and are locked here.
          </p>
        </div>
        <Button size="sm" onClick={startAdd}><Icon icon="plus" />Add local rule</Button>
      </div>

      {localRules.map((rule) => (
        <RuleRow
          key={`local-${rule.id}`}
          rule={rule}
          source="Local"
          onToggle={(enabled) => onChangeLocalRules(localRules.map((item) =>
            item.id === rule.id ? { ...item, enabled, revision: item.revision + 1 } : item
          ))}
          onEdit={() => startEdit(rule)}
          onRemove={() => removeRule(rule.id)}
        />
      ))}
      {fleetRules.map((rule) => (
        <RuleRow key={`fleet-${rule.id}`} rule={rule} source="Fleet" />
      ))}
      {localRules.length === 0 && fleetRules.length === 0 && (
        <p className="rounded border border-[var(--shield-inner-border)] px-3 py-2 text-[10px] text-[var(--shield-text-muted)]">
          No custom rules yet. WinCommander's built-in secret checks remain active.
        </p>
      )}

      {draft && (
        <RuleForm rule={draft} error={error} onChange={setDraft} onSave={saveDraft} onCancel={() => setDraft(null)} />
      )}
    </div>
  );
}

function RuleRow({
  rule,
  source,
  onToggle,
  onEdit,
  onRemove,
}: {
  rule: Rule;
  source: "Local" | "Fleet";
  onToggle?: (enabled: boolean) => void | Promise<void>;
  onEdit?: () => void;
  onRemove?: () => void | Promise<void>;
}) {
  const isFleet = source === "Fleet";
  return (
    <div className="flex items-start justify-between gap-3 rounded border border-[var(--shield-inner-border)] bg-[var(--color-bg-secondary)] px-3 py-2">
      <div className="min-w-0">
        <div className="flex items-center gap-2">
          <span className="truncate text-[11px] font-medium text-[var(--shield-text-subtle)]">{rule.name}</span>
          <span className="font-mono text-[9px] uppercase text-[var(--shield-text-muted)]">{source}</span>
          {isFleet && <Icon icon="lock" size={10} color="var(--shield-text-muted)" />}
        </div>
        <p className="mt-0.5 truncate font-mono text-[9px] text-[var(--shield-text-muted)]">
          {rule.matcher.kind} · {editableMatcherValue(rule.matcher)} · {rule.severity}
        </p>
      </div>
      <div className="flex shrink-0 items-center gap-1">
        <Switch checked={rule.enabled} disabled={isFleet} aria-label={`${source} rule ${rule.name}`} onCheckedChange={onToggle} />
        {!isFleet && <Button size="icon" variant="ghost" aria-label={`Edit ${rule.name}`} onClick={onEdit}><Icon icon="edit" /></Button>}
        {!isFleet && <Button size="icon" variant="ghost" aria-label={`Remove ${rule.name}`} onClick={onRemove}><Icon icon="trash" /></Button>}
      </div>
    </div>
  );
}

function RuleForm({ rule, error, onChange, onSave, onCancel }: {
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
          checked={rule.actions.includes("clear_clipboard")}
          label="Clear clipboard"
          onCheckedChange={(checked) => onChange(setLocalAction(rule, "clear_clipboard", checked))}
        />
        <LocalActionOption
          checked={rule.actions.includes("quarantine_clipboard")}
          label="Replace with a warning"
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
