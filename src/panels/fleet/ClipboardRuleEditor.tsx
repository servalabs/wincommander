import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";
import type { Action } from "@/types/generated/fleet";
import type { ClipboardRule, ClipboardSeverity } from "./fleetAdminTypes";
import SecurityField from "./SecurityField";

const ACTIONS: Array<{ value: Action; label: string }> = [
  { value: "notify_user", label: "Notify user" },
  { value: "clear_clipboard", label: "Clear" },
  { value: "quarantine_clipboard", label: "Quarantine" },
  { value: "record_local_receipt", label: "Local receipt" },
  { value: "report_fleet", label: "Report" },
  { value: "alert_admin", label: "Alert admin" },
];

export default function ClipboardRuleEditor({ rule, onChange, onRemove }: {
  rule: ClipboardRule;
  onChange: (rule: ClipboardRule) => void;
  onRemove: () => void;
}) {
  const matcherKind = rule.matcher.kind;
  const editableMatcher = matcherKind === "phrase" || matcherKind === "regex";
  const pattern = matcherKind === "phrase" ? rule.matcher.params.value
    : matcherKind === "regex" ? rule.matcher.params.pattern : "";
  const caseSensitive = matcherKind === "phrase" || matcherKind === "regex"
    ? rule.matcher.params.case_sensitive : false;
  const setPattern = (value: string) => onChange({ ...rule, matcher: matcherKind === "regex"
    ? { kind: "regex", params: { pattern: value, case_sensitive: caseSensitive } }
    : { kind: "phrase", params: { value, case_sensitive: caseSensitive } } });

  return (
    <section className="fleet-subcard">
      <div className="fleet-subcard-heading">
        <label className="fleet-rule-enabled"><Switch checked={rule.enabled} onCheckedChange={enabled => onChange({ ...rule, enabled })} disabled={rule.locked} /> Enabled</label>
        <Button size="sm" variant="danger" onClick={onRemove} disabled={rule.locked}>Remove</Button>
      </div>
      <div className="fleet-form-grid fleet-form-grid-4">
        <SecurityField label="Rule name"><Input value={rule.name} onChange={event => onChange({ ...rule, name: event.target.value })} disabled={rule.locked} /></SecurityField>
        <SecurityField label="Matcher">
          <Select value={matcherKind} disabled={rule.locked} onValueChange={kind => onChange({ ...rule, matcher: kind === "regex"
            ? { kind: "regex", params: { pattern, case_sensitive: caseSensitive } }
            : { kind: "phrase", params: { value: pattern, case_sensitive: caseSensitive } } })}>
            <SelectTrigger><SelectValue /></SelectTrigger><SelectContent>
              <SelectItem value="phrase">Phrase</SelectItem><SelectItem value="regex">Regex</SelectItem>
            </SelectContent>
          </Select>
        </SecurityField>
        <SecurityField label={!editableMatcher ? "Managed matcher" : matcherKind === "regex" ? "Rust regex" : "Phrase"}><Input value={editableMatcher ? pattern : `${matcherKind}: ${String(rule.matcher.params)}`} onChange={event => setPattern(event.target.value)} disabled={rule.locked || !editableMatcher} /></SecurityField>
        <SecurityField label="Severity">
          <Select value={rule.severity} disabled={rule.locked} onValueChange={severity => onChange({ ...rule, severity: severity as ClipboardSeverity })}>
            <SelectTrigger><SelectValue /></SelectTrigger><SelectContent>
              {(["info", "warn", "high", "critical"] as ClipboardSeverity[]).map(value => <SelectItem key={value} value={value}>{value}</SelectItem>)}
            </SelectContent>
          </Select>
        </SecurityField>
        <SecurityField label="Priority"><Input type="number" min={0} max={1000} value={rule.priority} onChange={event => onChange({ ...rule, priority: Number(event.target.value) })} disabled={rule.locked} /></SecurityField>
        <SecurityField label="Cooldown (seconds)"><Input type="number" min={0} max={86400} value={rule.cooldownSeconds} onChange={event => onChange({ ...rule, cooldownSeconds: Number(event.target.value) })} disabled={rule.locked} /></SecurityField>
      </div>
      <div className="fleet-policy-row">
        <label><input type="checkbox" checked={caseSensitive} disabled={rule.locked || !editableMatcher} onChange={event => onChange({ ...rule, matcher: matcherKind === "regex" ? { kind: "regex", params: { pattern, case_sensitive: event.target.checked } } : { kind: "phrase", params: { value: pattern, case_sensitive: event.target.checked } } })} /> Case sensitive</label>
        <label><input type="checkbox" checked={rule.snoozable} disabled={rule.locked} onChange={event => onChange({ ...rule, snoozable: event.target.checked })} /> User snooze</label>
        {ACTIONS.map(action => <label key={action.value}><input type="checkbox" checked={rule.actions.includes(action.value)} disabled={rule.locked} onChange={event => onChange({ ...rule, actions: event.target.checked ? [...rule.actions, action.value] : rule.actions.filter(value => value !== action.value) })} /> {action.label}</label>)}
      </div>
    </section>
  );
}
