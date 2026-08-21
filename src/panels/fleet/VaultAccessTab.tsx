import { useCallback, useEffect, useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import useVaultAccess from "@/hooks/useVaultAccess";
import { showError, showSuccess } from "@/utils/toast";
import { readUntrustedLegacyVaultDraft } from "./vaultLegacyImport";
import {
  newVaultEntry, newVaultPolicy, validateVaultAccessIntent,
  type VaultAccessEntry, type VaultAccessPolicy, type VaultEntryStatus, type VaultPolicyStatus,
} from "./vaultAccessTypes";

function observedResult(status: VaultEntryStatus | undefined) {
  if (!status) return "Not observed by the service";
  return status.result === "pending_mount_broker"
    ? "Host policy applied; secure mount broker pending"
    : status.result.replaceAll("_", " ");
}

function appliedAt(timestamp: number | null) {
  return timestamp == null ? "Never" : new Date(timestamp * 1000).toLocaleString();
}

export default function VaultAccessTab() {
  const [policy, setPolicy] = useState<VaultAccessPolicy | null>(null);
  const [status, setStatus] = useState<VaultPolicyStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [legacyNotice, setLegacyNotice] = useState<string | null>(null);
  const { getPolicy, getStatus, applyPolicy } = useVaultAccess<VaultAccessPolicy, VaultPolicyStatus>();
  const error = useMemo(() => policy ? validateVaultAccessIntent(policy) : null, [policy]);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [loadedPolicy, loadedStatus] = await Promise.all([
        getPolicy(),
        getStatus(),
      ]);
      setPolicy(loadedPolicy);
      setStatus(loadedStatus);
    } catch (cause) {
      setPolicy(null);
      setStatus(null);
      showError(cause instanceof Error ? cause.message : "Vault policy service is unavailable.");
    } finally {
      setLoading(false);
    }
  }, [getPolicy, getStatus]);

  useEffect(() => { void refresh(); }, [refresh]);

  const updateEntry = (id: string, patch: Partial<VaultAccessEntry>) => setPolicy(current => {
    const source = current ?? newVaultPolicy();
    return { ...source, entries: source.entries.map(entry => entry.id === id ? { ...entry, ...patch } : entry) };
  });

  const apply = async () => {
    if (!policy) return;
    if (error) return void showError(error);
    setSaving(true);
    try {
      await applyPolicy({
        // The service's optimistic lock accepts only the next revision. The
        // displayed version remains the last observed policy until refresh.
        ...policy,
        expected_previous_version: policy.version,
        version: policy.version + 1,
      });
      await refresh();
      showSuccess("Vault access intent was submitted and Windows ACL read-back was refreshed.");
    } catch (cause) {
      showError(cause instanceof Error ? cause.message : "Vault policy was not applied.");
    } finally {
      setSaving(false);
    }
  };

  const importLegacyDraft = () => {
    const entries = readUntrustedLegacyVaultDraft();
    if (!entries) return void setLegacyNotice("No retired local planner draft was found.");
    setPolicy(current => ({ ...(current ?? newVaultPolicy()), entries }));
    setLegacyNotice("Imported as an untrusted draft. Review container paths and grants before applying.");
  };

  if (loading) return <div className="fleet-admin-stack">Loading service-owned Vault policy…</div>;
  const activePolicy = policy;
  const statusById = new Map(status?.entries.map(entry => [entry.id, entry]));

  return (
    <div className="fleet-admin-stack">
      <Card>
        <CardHeader>
          <CardTitle>Vault access</CardTitle>
          <CardDescription>
            Requested access is intent. The SYSTEM service resolves Windows accounts, applies ACLs, and reports what it observed.
          </CardDescription>
        </CardHeader>
        <CardContent className="fleet-action-row">
          <Button onClick={() => setPolicy(current => current ?? newVaultPolicy())}>Create three-vault starter</Button>
          <Button variant="outline" onClick={importLegacyDraft}>Import retired planner as draft</Button>
          <Button variant="outline" onClick={() => void refresh()}>Refresh observed status</Button>
          {legacyNotice && <span className="fleet-field-hint">{legacyNotice}</span>}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Requested policy</CardTitle>
          <CardDescription>File containers only. Each managed container needs its own dedicated parent folder, so Windows can apply one unambiguous folder ACL. Account and group names are requests for the service to resolve; this screen never stores SIDs or ACLs.</CardDescription>
        </CardHeader>
        <CardContent className="fleet-admin-stack">
          {!activePolicy && <p className="fleet-field-hint">No service policy exists yet. Create the three-vault starter or import a retired planner as a draft.</p>}
          {activePolicy?.entries.map((entry, entryIndex) => {
            const observed = statusById.get(entry.id);
            return <div className="fleet-vault-workspace" key={entry.id}>
              <div className="fleet-vault-workspace-header">
                <strong>Vault {entryIndex + 1}</strong>
                <Button variant="outline" size="sm" onClick={() => setPolicy(current => current && ({ ...current, entries: current.entries.filter(item => item.id !== entry.id) }))}>Remove</Button>
              </div>
              <div className="fleet-owner-inputs">
                <Input aria-label={`Vault ${entryIndex + 1} label`} value={entry.label} placeholder="Vault label" onChange={event => updateEntry(entry.id, { label: event.target.value })} />
                <Input aria-label={`Vault ${entryIndex + 1} container path`} value={entry.container_path} placeholder="C:\\Vaults\\shared.hc" onChange={event => updateEntry(entry.id, { container_path: event.target.value })} />
                <Input aria-label={`Vault ${entryIndex + 1} owner`} value={entry.owner_account} placeholder="Administrator account" onChange={event => updateEntry(entry.id, { owner_account: event.target.value })} />
                <label className="fleet-field"><span>Presentation</span><select value={entry.mount.presentation} onChange={event => updateEntry(entry.id, { mount: { ...entry.mount, presentation: event.target.value as VaultAccessEntry["mount"]["presentation"] } })}><option value="machine">Machine (shared)</option><option value="per-user">Per-user (private)</option></select></label>
                <Input aria-label={`Vault ${entryIndex + 1} preferred drive letter`} value={entry.mount.preferred_letter ?? ""} maxLength={1} placeholder="Preferred letter" onChange={event => updateEntry(entry.id, { mount: { ...entry.mount, preferred_letter: event.target.value.toUpperCase() || undefined } })} />
              </div>
              <div className="fleet-vault-matrix">
                <strong>Named grants (intent)</strong>
                {entry.grants.map((grant, grantIndex) => <div className="fleet-action-row" key={`${entry.id}-${grantIndex}`}>
                  <Input aria-label={`Grant ${grantIndex + 1} principal`} value={grant.principal_name} placeholder="Windows account or group" onChange={event => updateEntry(entry.id, { grants: entry.grants.map((current, index) => index === grantIndex ? { ...current, principal_name: event.target.value } : current) })} />
                  <select aria-label={`Grant ${grantIndex + 1} access`} value={grant.access} onChange={event => updateEntry(entry.id, { grants: entry.grants.map((current, index) => index === grantIndex ? { ...current, access: event.target.value as "read" | "write" } : current) })}><option value="write">Read & write</option><option value="read">Read only</option></select>
                  <Button variant="outline" size="sm" onClick={() => updateEntry(entry.id, { grants: entry.grants.filter((_, index) => index !== grantIndex) })}>Remove grant</Button>
                </div>)}
                <Button variant="outline" size="sm" onClick={() => updateEntry(entry.id, { grants: [...entry.grants, { principal_name: "", access: "write" }] })}>Add grant</Button>
              </div>
              <p className="fleet-field-hint">Observed: {observedResult(observed)}. Mount launch is intentionally unavailable until the service owns secure mount → ACL → read-back → presentation.</p>
            </div>;
          })}
          <div className="fleet-action-row">
            <Button variant="outline" onClick={() => setPolicy(current => {
              const source = current ?? newVaultPolicy();
              return { ...source, entries: [...source.entries, newVaultEntry()] };
            })}>Add private vault</Button>
            <Button variant="outline" onClick={() => setPolicy(current => {
              const source = current ?? newVaultPolicy();
              return { ...source, entries: [...source.entries, newVaultEntry("shared")] };
            })}>Add shared vault</Button>
            <Button variant="primary" disabled={saving || !!error} onClick={() => void apply()}>{saving ? "Applying…" : "Apply requested policy"}</Button>
          </div>
          {error && <p className="fleet-validation-errors">{error}</p>}
        </CardContent>
      </Card>

      <Card>
        <CardHeader><CardTitle>Observed service status</CardTitle><CardDescription>Windows-confirmed ACL state, not a forecast. Secure mount enforcement remains pending in this one-day slice.</CardDescription></CardHeader>
        <CardContent>
          <p>Policy: {status?.policy_id ?? "none"} · Version {status?.version ?? 0} · Validation: {status?.validation_state ?? "never_applied"} · Applied: {appliedAt(status?.applied_at ?? null)}</p>
          <ul className="fleet-validation-errors">
            {status?.entries.map(entry => <li key={entry.id}>{entry.id}: {observedResult(entry)}</li>)}
          </ul>
        </CardContent>
      </Card>
    </div>
  );
}
