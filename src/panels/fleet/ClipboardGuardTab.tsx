import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { showError, showSuccess } from "@/utils/toast";
import {
  getClipboardRules, getOrgSettings, publishClipboardRules, saveClipboardRules,
  testClipboardRules, updateOrgSettings,
} from "./fleetAdminApi";
import { useFleetSession } from "./FleetAdminSession";
import ClipboardRuleEditor from "./ClipboardRuleEditor";
import type { ClipboardRule } from "./fleetAdminTypes";

const newRule = (): ClipboardRule => ({
  id: crypto.randomUUID(), revision: 0, name: "New clipboard rule", enabled: true,
  priority: 500, matcher: { kind: "phrase", params: { value: "", case_sensitive: false } },
  severity: "high", actions: ["notify_user", "report_fleet"], cooldownSeconds: 60,
  snoozable: false, locked: false,
});

export default function ClipboardGuardTab() {
  const { session } = useFleetSession();
  const [enabled, setEnabled] = useState(false);
  const [rules, setRules] = useState<ClipboardRule[]>([]);
  const [syntheticText, setSyntheticText] = useState("");
  const [result, setResult] = useState("");
  const [busy, setBusy] = useState(true);

  useEffect(() => {
    if (!session) return;
    Promise.all([getOrgSettings(session), getClipboardRules(session)])
      .then(([settings, current]) => { setEnabled(settings.clipboard_guard_enabled); setRules(current); })
      .catch(cause => void showError(cause instanceof Error ? cause.message : String(cause)))
      .finally(() => setBusy(false));
  }, [session]);

  if (!session) return null;
  const mutate = session.role === "admin" || session.role === "super_admin";
  const updateRule = (next: ClipboardRule) => setRules(current => current.map(rule => rule.id === next.id ? next : rule));
  const save = async () => {
    setBusy(true);
    try {
      const stored = await saveClipboardRules(session, rules);
      await updateOrgSettings(session, { clipboard_guard_enabled: enabled });
      setRules(stored); void showSuccess("Clipboard Guard draft and feature state saved.");
    } catch (cause) { void showError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setBusy(false); }
  };
  const test = async () => {
    setBusy(true);
    try {
      const response = await testClipboardRules(session, rules, syntheticText);
      setResult(response.compiled ? (response.verdict ? `Matched: ${JSON.stringify(response.verdict)}` : "Compiled; no rule matched.") : response.errors.join(" · "));
    } catch (cause) { setResult(cause instanceof Error ? cause.message : String(cause)); }
    finally { setBusy(false); }
  };
  const publish = async () => {
    setBusy(true);
    try { const epoch = await publishClipboardRules(session); void showSuccess(`Clipboard Guard published as policy epoch ${epoch.version}.`); }
    catch (cause) { void showError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setBusy(false); }
  };

  return <div className="fleet-admin-stack">
    <Card><CardHeader><CardTitle>Clipboard Guard policy</CardTitle><CardDescription>Author content-matching rules, compile them with the endpoint’s Rust engine, then explicitly publish.</CardDescription></CardHeader>
      <CardContent>
        <label className="fleet-switch-field"><Switch checked={enabled} onCheckedChange={setEnabled} disabled={!mutate} /><span><strong>Enable Clipboard Guard</strong><small>Real clipboard text never goes to Fleet; only content-free match receipts do.</small></span></label>
        <div className="fleet-volume-stack">{rules.map(rule => <ClipboardRuleEditor key={rule.id} rule={rule} onChange={updateRule} onRemove={() => setRules(current => current.filter(item => item.id !== rule.id))} />)}</div>
        <Button onClick={() => setRules(current => [...current, newRule()])} disabled={!mutate}>Add rule</Button>
      </CardContent>
    </Card>
    <Card><CardHeader><CardTitle>Compile and publish</CardTitle><CardDescription>Use synthetic text only. Saving a draft does not change endpoint behavior until Publish.</CardDescription></CardHeader>
      <CardContent><Input value={syntheticText} onChange={event => setSyntheticText(event.target.value)} placeholder="Synthetic clipboard text" />
        {result && <p className="fleet-test-result">{result}</p>}
        <div className="fleet-action-row"><Button onClick={test} disabled={busy}>Compile & test</Button><Button onClick={save} disabled={busy || !mutate}>Save draft</Button><Button variant="primary" onClick={publish} disabled={busy || !mutate}>Publish</Button></div>
      </CardContent>
    </Card>
  </div>;
}
