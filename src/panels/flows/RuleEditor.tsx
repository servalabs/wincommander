// RuleEditor — a modal form for creating / editing a v2 flow rule.
//
// Renders one section per block family (triggers → conditions → actions). The
// CommandAction picker is backed by the full app command catalog (a native
// datalist), so a flow can drive ANY in-app command — destructive ones
// included; the Pro classifier still guarantees the command is recognized.

import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Icon } from "@/components/ui/icon";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  ACTION_LABELS,
  GAZE_KIND_LABELS,
  TOGGLE_CATALOG,
  TRIGGER_LABELS,
  defaultAction,
  defaultTrigger,
  isFleetLocked,
  type Action,
  type ActionType,
  type Condition,
  type GazeKind,
  type Rule,
  type Trigger,
  type TriggerType,
} from "./rules";
import {
  DEFAULT_FLOW_SETTING_OPTIONS,
  formatFlowSettingValue,
  parseFlowSettingValue,
  type FlowSettingOption,
} from "./settingsCatalog";

interface Props {
  rule: Rule;
  commands: string[];
  settingOptions: FlowSettingOption[];
  onSave(rule: Rule): Promise<void>;
  onClose(): void;
}

function Sel<T extends string>({
  value,
  onChange,
  options,
  ariaLabel,
  disabled,
}: {
  value: T;
  onChange: (v: T) => void;
  options: { value: T; label: string }[];
  ariaLabel: string;
  disabled?: boolean;
}) {
  return (
    <Select value={value} onValueChange={(v) => onChange(v as T)} disabled={disabled}>
      <SelectTrigger className="min-w-[120px]" aria-label={ariaLabel}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {options.map((o) => (
          <SelectItem key={o.value} value={o.value}>
            {o.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

export default function RuleEditor({ rule, commands, settingOptions, onSave, onClose }: Props) {
  const [draft, setDraft] = useState<Rule>(rule);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const locked = isFleetLocked(draft);

  const patch = (p: Partial<Rule>) => setDraft((d) => ({ ...d, ...p }));

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      await onSave(draft);
      onClose();
    } catch (e) {
      setError(String(e));
      setSaving(false);
    }
  };

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="flow-editor">
        <DialogHeader>
          <DialogTitle>{rule.name === "New flow" ? "New flow" : "Edit flow"}</DialogTitle>
        </DialogHeader>

        {locked && (
          <div className="flow-editor__locked">
            <Icon icon="lock" size={13} /> Managed by fleet policy — read-only.
          </div>
        )}

        <div className="flow-editor__body">
          <datalist id="flow-setting-path-list">
            {settingOptions.map((setting) => (
              <option key={setting.path} value={setting.path}>
                {setting.label}
              </option>
            ))}
          </datalist>

          <label className="flow-field">
            <span className="flow-field__label">Name</span>
            <Input
              value={draft.name}
              disabled={locked}
              onChange={(e) => patch({ name: e.target.value })}
            />
          </label>

          {/* ── WHEN ── */}
          <Section
            title="When"
            icon="flash"
            onAdd={locked ? undefined : () => patch({ triggers: [...draft.triggers, defaultTrigger("GazeTrigger")] })}
            addLabel="Add trigger"
          >
            {draft.triggers.map((t, i) => (
              <BlockRow key={i} onRemove={locked || draft.triggers.length === 1 ? undefined : () => patch({ triggers: draft.triggers.filter((_, j) => j !== i) })}>
                <TriggerEditor
                  trigger={t}
                  settingOptions={settingOptions}
                  disabled={locked}
                  onChange={(nt) => patch({ triggers: draft.triggers.map((x, j) => (j === i ? nt : x)) })}
                />
              </BlockRow>
            ))}
          </Section>

          {/* ── ONLY IF (conditions) ── */}
          <Section
            title="Only if"
            icon="filter"
            optional
            onAdd={locked ? undefined : () => patch({ conditions: [...draft.conditions, { type: "TimeCondition", startHour: 22, endHour: 6 }] })}
            addLabel="Add condition"
          >
            {draft.conditions.length === 0 && <p className="flow-empty">Always (no conditions).</p>}
            {draft.conditions.map((c, i) => (
              <BlockRow key={i} onRemove={locked ? undefined : () => patch({ conditions: draft.conditions.filter((_, j) => j !== i) })}>
                <ConditionEditor
                  condition={c}
                  settingOptions={settingOptions}
                  disabled={locked}
                  onChange={(nc) => patch({ conditions: draft.conditions.map((x, j) => (j === i ? nc : x)) })}
                />
              </BlockRow>
            ))}
          </Section>

          {/* ── DO ── */}
          <Section
            title="Do"
            icon="play"
            onAdd={locked ? undefined : () => patch({ actions: [...draft.actions, defaultAction("NotifyAction")] })}
            addLabel="Add action"
          >
            {draft.actions.map((a, i) => (
              <BlockRow key={i} onRemove={locked || draft.actions.length === 1 ? undefined : () => patch({ actions: draft.actions.filter((_, j) => j !== i) })}>
                <ActionEditor
                  action={a}
                  commands={commands}
                  disabled={locked}
                  onChange={(na) => patch({ actions: draft.actions.map((x, j) => (j === i ? na : x)) })}
                />
              </BlockRow>
            ))}
          </Section>
        </div>

        {error && <p className="flow-editor__error">{error}</p>}

        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={save} disabled={saving || locked}>
            {saving ? "Saving…" : "Save flow"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function Section({
  title,
  icon,
  optional,
  onAdd,
  addLabel,
  children,
}: {
  title: string;
  icon: string;
  optional?: boolean;
  onAdd?: () => void;
  addLabel: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flow-section">
      <div className="flow-section__head">
        <span className="flow-section__title">
          <Icon icon={icon} size={13} /> {title}
          {optional && <span className="flow-section__opt">optional</span>}
        </span>
        {onAdd && (
          <button type="button" className="flow-addbtn" onClick={onAdd}>
            <Icon icon="plus" size={12} /> {addLabel}
          </button>
        )}
      </div>
      <div className="flow-section__body">{children}</div>
    </div>
  );
}

function BlockRow({ children, onRemove }: { children: React.ReactNode; onRemove?: () => void }) {
  return (
    <div className="flow-block">
      <div className="flow-block__main">{children}</div>
      {onRemove && (
        <button type="button" className="flow-block__remove" aria-label="Remove" onClick={onRemove}>
          <Icon icon="cross" size={12} />
        </button>
      )}
    </div>
  );
}

function TriggerEditor({
  trigger,
  disabled,
  settingOptions,
  onChange,
}: {
  trigger: Trigger;
  disabled: boolean;
  settingOptions: FlowSettingOption[];
  onChange: (t: Trigger) => void;
}) {
  const types: TriggerType[] = [
    "SettingChangedTrigger",
    "GazeTrigger",
    "USBTrigger",
    "RansomwareMonitorTrigger",
    "PasteMonitorTrigger",
    "DecoyMonitorTrigger",
    "WifiGuardTrigger",
  ];
  return (
    <div className="flow-block__grid">
      <Sel
        ariaLabel="Trigger type"
        value={trigger.type}
        onChange={(v) => onChange(defaultTrigger(v))}
        options={types.map((t) => ({ value: t, label: TRIGGER_LABELS[t] }))}
      />
      {trigger.type === "SettingChangedTrigger" && (
        <>
          <SettingPathInput
            value={trigger.path}
            disabled={disabled}
            onChange={(path) => {
              const known = settingOptions.find((setting) => setting.path === path);
              onChange({ type: "SettingChangedTrigger", path, to: known?.value });
            }}
          />
          <Sel
            ariaLabel="Setting change match"
            disabled={disabled}
            value={trigger.to === undefined ? "any" : "equals"}
            onChange={(mode) =>
              onChange({
                ...trigger,
                to:
                  mode === "any"
                    ? undefined
                    : (settingOptions.find((setting) => setting.path === trigger.path)?.value ?? true),
              })
            }
            options={[
              { value: "any", label: "changes to any value" },
              { value: "equals", label: "changes to value" },
            ]}
          />
          {trigger.to !== undefined && (
            <SettingValueInput
              key={`${trigger.path}:${formatFlowSettingValue(trigger.to)}`}
              value={trigger.to}
              disabled={disabled}
              onChange={(to) => onChange({ ...trigger, to })}
            />
          )}
        </>
      )}
      {trigger.type === "GazeTrigger" && (
        <Sel
          ariaLabel="Gaze kind"
          value={(trigger.kind ?? "look_away") as GazeKind}
          onChange={(kind) => onChange({ type: "GazeTrigger", kind })}
          options={(Object.keys(GAZE_KIND_LABELS) as GazeKind[]).map((k) => ({ value: k, label: GAZE_KIND_LABELS[k] }))}
        />
      )}
      {trigger.type === "USBTrigger" && (
        <Sel
          ariaLabel="USB mode"
          value={trigger.mode}
          onChange={(mode) => onChange({ type: "USBTrigger", mode: mode as "insert" | "remove" })}
          options={[
            { value: "remove", label: "removed" },
            { value: "insert", label: "inserted" },
          ]}
        />
      )}
      {disabled && <input type="hidden" />}
    </div>
  );
}

function ConditionEditor({
  condition,
  disabled,
  settingOptions,
  onChange,
}: {
  condition: Condition;
  disabled: boolean;
  settingOptions: FlowSettingOption[];
  onChange: (c: Condition) => void;
}) {
  return (
    <div className="flow-block__grid">
      <Sel
        ariaLabel="Condition type"
        value={condition.type}
        onChange={(v) => {
          if (v === "TimeCondition") onChange({ type: "TimeCondition", startHour: 22, endHour: 6 });
          else if (v === "BatteryCondition") onChange({ type: "BatteryCondition", operator: "<", percentage: 20 });
          else {
            const first = settingOptions[0] ?? DEFAULT_FLOW_SETTING_OPTIONS[0];
            onChange({ type: "SettingCondition", path: first.path, operator: "==", value: first.value });
          }
        }}
        options={[
          { value: "TimeCondition", label: "Time of day is between" },
          { value: "BatteryCondition", label: "Battery is" },
          { value: "SettingCondition", label: "A setting equals" },
        ]}
      />
      {condition.type === "TimeCondition" && (
        <span className="flow-inline">
          <Input type="number" min={0} max={23} disabled={disabled} value={condition.startHour} onChange={(e) => onChange({ ...condition, startHour: Number(e.target.value) })} />
          <span>–</span>
          <Input type="number" min={0} max={23} disabled={disabled} value={condition.endHour} onChange={(e) => onChange({ ...condition, endHour: Number(e.target.value) })} />
          <span className="flow-hint">h</span>
        </span>
      )}
      {condition.type === "BatteryCondition" && (
        <span className="flow-inline">
          <Sel ariaLabel="op" value={condition.operator} onChange={(operator) => onChange({ ...condition, operator: operator as Condition["type"] extends "BatteryCondition" ? "<" : never })} options={[{ value: "<", label: "<" }, { value: "<=", label: "≤" }, { value: ">", label: ">" }, { value: ">=", label: "≥" }].map((o) => ({ value: o.value as "<", label: o.label }))} />
          <Input type="number" min={0} max={100} disabled={disabled} value={condition.percentage} onChange={(e) => onChange({ ...condition, percentage: Number(e.target.value) })} />
          <span className="flow-hint">%</span>
        </span>
      )}
      {condition.type === "SettingCondition" && (
        <>
          <SettingPathInput
            value={condition.path}
            disabled={disabled}
            onChange={(path) => {
              const known = settingOptions.find((setting) => setting.path === path);
              onChange({ ...condition, path, value: known?.value ?? condition.value });
            }}
          />
          <Sel
            ariaLabel="Setting comparison"
            disabled={disabled}
            value={condition.operator}
            onChange={(operator) => onChange({ ...condition, operator })}
            options={[
              { value: "==", label: "equals" },
              { value: "!=", label: "does not equal" },
            ]}
          />
          <SettingValueInput
            key={`${condition.path}:${formatFlowSettingValue(condition.value)}`}
            value={condition.value}
            disabled={disabled}
            onChange={(value) => onChange({ ...condition, value })}
          />
        </>
      )}
    </div>
  );
}

function SettingPathInput({
  value,
  disabled,
  onChange,
}: {
  value: string;
  disabled: boolean;
  onChange(path: string): void;
}) {
  return (
    <Input
      list="flow-setting-path-list"
      aria-label="Setting path"
      placeholder="Search or enter any setting path…"
      disabled={disabled}
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  );
}

function SettingValueInput({
  value,
  disabled,
  onChange,
}: {
  value: unknown;
  disabled: boolean;
  onChange(value: unknown): void;
}) {
  const [text, setText] = useState(() => formatFlowSettingValue(value));

  return (
    <Input
      aria-label="Setting value"
      placeholder='Value, e.g. true, 42, or "Allow"'
      disabled={disabled}
      value={text}
      onChange={(event) => setText(event.target.value)}
      onBlur={() => onChange(parseFlowSettingValue(text))}
    />
  );
}

function ActionEditor({ action, commands, disabled, onChange }: { action: Action; commands: string[]; disabled: boolean; onChange: (a: Action) => void }) {
  const types: ActionType[] = [
    "SetToggleAction",
    "CommandAction",
    "NotifyAction",
    "DelayAction",
    "SignalAction",
    "LockdownAction",
  ];
  return (
    <div className="flow-block__grid">
      <Sel
        ariaLabel="Action type"
        value={action.type}
        onChange={(v) => onChange(defaultAction(v))}
        options={types.map((t) => ({ value: t, label: ACTION_LABELS[t] }))}
      />
      {action.type === "SetToggleAction" && (() => {
        const meta = TOGGLE_CATALOG.find((t) => t.id === action.toggleId);
        // Pick the OUTCOME directly instead of an ambiguous on/off switch. A bare
        // "on/off" reads backwards for capability toggles (on = protection engaged
        // = the resource is DENIED), so users would set "off" wanting the mic off
        // and actually ALLOW it. Two explicit outcomes ("mic denied" / "mic
        // allowed") make what-you-set == what-happens. Value maps to the `on`
        // bool: the protective/deny outcome is `on: true`.
        const denyLabel = meta?.onMeans ?? "protection engaged";
        const allowLabel = meta?.offMeans ?? "protection off";
        return (
          <span className="flow-inline">
            <Sel ariaLabel="Toggle" disabled={disabled} value={action.toggleId} onChange={(toggleId) => onChange({ ...action, toggleId })} options={TOGGLE_CATALOG.map((t) => ({ value: t.id, label: t.label }))} />
            <span className="flow-hint">→ set to</span>
            <Sel
              ariaLabel="Effect"
              disabled={disabled}
              value={action.on ? "on" : "off"}
              onChange={(v) => onChange({ ...action, on: v === "on" })}
              options={[
                { value: "on", label: denyLabel },
                { value: "off", label: allowLabel },
              ]}
            />
          </span>
        );
      })()}
      {action.type === "CommandAction" && (
        <>
          <Input
            list="flow-command-list"
            placeholder="Search any app command…"
            disabled={disabled}
            value={action.command}
            onChange={(e) => onChange({ ...action, command: e.target.value })}
          />
          <datalist id="flow-command-list">
            {commands.map((c) => (
              <option key={c} value={c} />
            ))}
          </datalist>
        </>
      )}
      {action.type === "NotifyAction" && (
        <span className="flow-inline">
          <Input placeholder="Message" disabled={disabled} value={action.message} onChange={(e) => onChange({ ...action, message: e.target.value })} />
          <Sel ariaLabel="Severity" value={action.severity} onChange={(severity) => onChange({ ...action, severity })} options={[{ value: "info", label: "info" }, { value: "warning", label: "warning" }, { value: "danger", label: "danger" }]} />
        </span>
      )}
      {action.type === "DelayAction" && (
        <span className="flow-inline">
          <Input type="number" min={0} disabled={disabled} value={action.seconds} onChange={(e) => onChange({ ...action, seconds: Number(e.target.value) })} />
          <span className="flow-hint">seconds</span>
        </span>
      )}
      {action.type === "SignalAction" && (
        <span className="flow-inline">
          <Input placeholder="Role" disabled={disabled} value={action.targetRole} onChange={(e) => onChange({ ...action, targetRole: e.target.value })} />
          <Input placeholder="Signal" disabled={disabled} value={action.signalType} onChange={(e) => onChange({ ...action, signalType: e.target.value })} />
        </span>
      )}
      {action.type === "LockdownAction" && (
        <span className="flow-danger-note">
          <Icon icon="warning-sign" size={12} /> Only fires if self-destruct is already armed.
        </span>
      )}
    </div>
  );
}
