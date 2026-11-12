import { useCallback, useMemo, useState } from "react";
import ContentDetectorCatalog from "./ContentDetectorCatalog";
import RuleForm from "./LocalClipboardRuleForm";
import { Button } from "@/components/ui/button";
import { Icon } from "@/components/ui/icon";
import { Switch } from "@/components/ui/switch";
import type { Rule } from "../../types/generated/fleet";
import type { ClipboardGuardRuleLoadState } from "../../hooks/useClipboardGuardRules";
import {
  canEditLocalClipboardRules,
  createLocalClipboardRule,
  editableMatcherValue,
  ensureLocalActions,
  validateLocalClipboardRule,
} from "./LocalClipboardRules";

interface Props {
  localRules: Rule[];
  fleetRules: Rule[];
  loadState?: ClipboardGuardRuleLoadState;
  onChangeLocalRules: (rules: Rule[]) => void | Promise<void>;
}

export default function LocalClipboardRulesEditor({
  localRules,
  fleetRules,
  loadState = "ready",
  onChangeLocalRules,
}: Props) {
  const [draft, setDraft] = useState<Rule | null>(null);
  const [error, setError] = useState<string | null>(null);
  const canEdit = canEditLocalClipboardRules(loadState);
  const [catalogIds, setCatalogIds] = useState<ReadonlySet<string>>(new Set());
  const receiveCatalog = useCallback((rules: Rule[]) => setCatalogIds(new Set(rules.map((rule) => rule.id))), []);
  const allIds = useMemo(
    () => new Set([...localRules, ...fleetRules].map((rule) => rule.id)),
    [fleetRules, localRules],
  );

  const persistRules = async (rules: Rule[]): Promise<boolean> => {
    try {
      await onChangeLocalRules(rules);
      setError(null);
      return true;
    } catch {
      // Backend failures are intentionally not surfaced verbatim: a matcher
      // value must never be reflected into a rendered error message.
      setError("Local rules could not be saved. Check that Clipboard Guard is available, then try again.");
      return false;
    }
  };

  const startAdd = () => {
    setError(null);
    setDraft(createLocalClipboardRule(allIds));
  };

  const startEdit = (rule: Rule) => {
    setError(null);
    // Older saved rules could contain no action. A detection with no response
    // is meaningless, so repair that legacy draft visibly before it is saved.
    setDraft({ ...rule, actions: ensureLocalActions(rule.actions), locked: false });
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
    const didSave = await persistRules(
      existed
        ? localRules.map((rule) => rule.id === saved.id ? saved : rule)
        : [...localRules, saved],
    );
    if (!didSave) return;
    setDraft(null);
    setError(null);
  };

  const removeRule = async (id: string) => {
    if (await persistRules(localRules.filter((rule) => rule.id !== id))) {
      if (draft?.id === id) setDraft(null);
    }
  };

  return (
    <div className="flex flex-col gap-3">
      <ContentDetectorCatalog localRules={localRules} fleetRules={fleetRules} disabled={!canEdit} onSave={persistRules} onCatalogLoaded={receiveCatalog} />
      <div className="flex items-center justify-between gap-3">
        <div>
          <div className="text-[10px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)]">
            Custom rules
          </div>
          <p className="mt-1 text-[10px] text-[var(--shield-text-muted)]">
            Local rules stay on this device. Fleet rules are managed by your organization and are locked here.
          </p>
        </div>
        <Button size="sm" disabled={!canEdit} onClick={startAdd}><Icon icon="plus" />Add local rule</Button>
      </div>

      {loadState === "loading" && (
        <p className="rounded border border-[var(--shield-inner-border)] px-3 py-2 text-[10px] text-[var(--shield-text-muted)]">
          Loading local rules…
        </p>
      )}
      {loadState === "degraded" && (
        <p role="alert" className="rounded border border-[var(--color-warning)]/50 px-3 py-2 text-[10px] text-[var(--color-warning)]">
          Local rules could not be loaded. Built-in secret checks may still run, but saved custom rules are unavailable until this is resolved.
        </p>
      )}
      {error && !draft && <p role="alert" className="text-[10px] text-[var(--color-danger)]">{error}</p>}

      {loadState === "ready" && localRules.filter((rule) => rule.matcher.kind === "phrase" || rule.matcher.kind === "regex").map((rule) => (
        <RuleRow
          key={`local-${rule.id}`}
          rule={rule}
          source="Local"
          onToggle={(enabled) => {
            void persistRules(localRules.map((item) =>
              item.id === rule.id ? { ...item, enabled, revision: item.revision + 1 } : item
            ));
          }}
          onEdit={() => startEdit(rule)}
          onRemove={() => removeRule(rule.id)}
        />
      ))}
      {fleetRules.filter((rule) => !catalogIds.has(rule.id)).map((rule) => (
        <RuleRow key={`fleet-${rule.id}`} rule={rule} source="Fleet" />
      ))}
      {loadState === "ready" && localRules.length === 0 && fleetRules.length === 0 && (
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
