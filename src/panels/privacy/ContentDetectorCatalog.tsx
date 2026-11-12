import { useEffect, useState } from "react";
import { loadContentDetectorCatalog, testLocalContentRules } from "../../hooks/useContentDetectors";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import type { Finding, Rule } from "../../types/generated/fleet";
import { describeFinding, localDetectorOverride, resolveDetectorRule } from "./contentDetectorModel";

interface Props {
  localRules: Rule[];
  fleetRules: Rule[];
  disabled: boolean;
  onSave: (rules: Rule[]) => Promise<boolean>;
  onCatalogLoaded: (rules: Rule[]) => void;
}

export default function ContentDetectorCatalog({ localRules, fleetRules, disabled, onSave, onCatalogLoaded }: Props) {
  const [catalog, setCatalog] = useState<Rule[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [sample, setSample] = useState("");
  const [findings, setFindings] = useState<Finding[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [truncated, setTruncated] = useState(false);
  const [filter, setFilter] = useState("");
  useEffect(() => {
    let active = true;
    loadContentDetectorCatalog().then((rules) => {
      if (active) { setCatalog(rules); onCatalogLoaded(rules); }
    }).catch(() => { if (active) setError("The detector catalog is unavailable."); });
    return () => { active = false; };
  }, [onCatalogLoaded]);

  const resolveRule = (preset: Rule) => resolveDetectorRule(preset, localRules, fleetRules);
  const update = async (preset: Rule, patch: Partial<Rule>) => {
    const current = resolveRule(preset);
    setBusy(true);
    try {
      const next = localDetectorOverride(current, patch);
      await onSave([...localRules.filter((rule) => !sameDetector(rule, preset)), next]);
    } finally { setBusy(false); }
  };
  const test = async () => {
    setBusy(true);
    setError(null);
    setFindings(null);
    try {
      const rules = [...catalog.map(resolveRule), ...localRules.filter((rule) => !catalog.some((preset) => sameDetector(rule, preset))),
        ...fleetRules.filter((rule) => !catalog.some((preset) => sameDetector(rule, preset)))];
      const result = await testLocalContentRules(rules, sample);
      setFindings(result.findings);
      setTruncated(result.truncated);
      setSample("");
    } catch { setError("The local test could not run. Check the rules and sample size."); }
    finally { setBusy(false); }
  };
  return (
    <div className="flex flex-col gap-2 rounded border border-[var(--shield-inner-border)] p-3">
      <div className="text-xs font-medium">Content detectors</div>
      <p className="text-[10px] text-[var(--shield-text-muted)]">Alerts are the default. Masking requires Pro. Organization rules are locked here.</p>
      <Input aria-label="Filter content detectors" placeholder="Find a detector" value={filter} onChange={(event) => setFilter(event.currentTarget.value)} />
      <div className="max-h-64 overflow-y-auto">
        {catalog.filter((preset) => preset.name.toLowerCase().includes(filter.toLowerCase())).map((preset) => {
          const rule = resolveRule(preset);
          const managed = fleetRules.some((item) => sameDetector(item, preset));
          return <div key={preset.id} className="flex items-center gap-2 border-b border-[var(--shield-inner-border)] py-2 text-xs">
            <Switch aria-label={`Enable ${preset.name}`} checked={rule.enabled} disabled={disabled || busy || managed}
              onCheckedChange={(enabled) => void update(preset, { enabled })} />
            <span className="flex-1">{preset.name}{managed ? " (Fleet)" : ""}</span>
            <Input className="w-16" aria-label={`${preset.name} threshold`} type="number" min={1} max={100}
              key={`${rule.id}-${rule.revision}`} defaultValue={String(rule.threshold)} disabled={disabled || busy || managed}
              onBlur={(event) => {
                const threshold = Number(event.currentTarget.value);
                if (Number.isInteger(threshold) && threshold >= 1 && threshold <= 100) void update(preset, { threshold });
              }} />
          </div>;
        })}
      </div>
      <label className="text-xs" htmlFor="content-detector-sample">Test locally: why it matched</label>
      <textarea id="content-detector-sample" className="rounded border border-[var(--shield-inner-border)] bg-[var(--color-bg-primary)] p-2 text-xs"
        value={sample} maxLength={65536} placeholder="Synthetic sample text" onChange={(event) => { setSample(event.currentTarget.value); setFindings(null); }} />
      <p className="text-[10px] text-[var(--shield-text-muted)]">The sample stays on this device and is cleared after testing. Results contain categories and reasons only.</p>
      <Button size="sm" disabled={disabled || busy || !sample || catalog.length === 0} onClick={() => void test()}>Test sample</Button>
      {error && <p role="alert" className="text-xs text-[var(--color-danger)]">{error}</p>}
      {truncated && findings !== null && <p role="status" className="text-xs">The scan reached its limit; these results are partial.</p>}
      {findings?.length === 0 && <p role="status" className="text-xs">No matching detectors.</p>}
      {findings?.map((finding) => <div key={finding.ruleId} className="text-xs">
        {finding.category.replaceAll("_", " ")} · {finding.count} match(es) · confidence {finding.score}/100
        <div className="text-[var(--shield-text-muted)]">{describeFinding(finding)}</div>
      </div>)}
    </div>
  );
}

function sameDetector(rule: Rule, preset: Rule): boolean {
  return rule.id === preset.id;
}
