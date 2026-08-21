import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { useAppState } from "@/context/AppContext";
import useVaultAccess from "@/hooks/useVaultAccess";
import { showError, showSuccess } from "@/utils/toast";
import { readUntrustedLegacyVaultDraft } from "./vaultLegacyImport";
import {
  newVaultEntry, newVaultPolicy, validateVaultAccessIntent, vaultMountResultLabel, vaultPresentationLabel,
  type VaultAuthorizedEntry,
  type VaultMountEntryResult,
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
  const { systemInfo } = useAppState();
  const isAdmin = systemInfo?.isAdmin === true;
  const [policy, setPolicy] = useState<VaultAccessPolicy | null>(null);
  const [status, setStatus] = useState<VaultPolicyStatus | null>(null);
  const [authorizedEntries, setAuthorizedEntries] = useState<VaultAuthorizedEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [legacyNotice, setLegacyNotice] = useState<string | null>(null);
  const [mountTargetId, setMountTargetId] = useState<string | null>(null);
  const [mountingEntryId, setMountingEntryId] = useState<string | null>(null);
  const [unmountingEntryId, setUnmountingEntryId] = useState<string | null>(null);
  const [mountResults, setMountResults] = useState<Record<string, VaultMountEntryResult>>({});
  const passwordInputRef = useRef<HTMLInputElement>(null);
  const { getPolicy, getStatus, applyPolicy, mountEntry, unmountEntry, listAuthorizedEntries } = useVaultAccess<VaultAccessPolicy, VaultPolicyStatus>();
  const error = useMemo(() => policy ? validateVaultAccessIntent(policy) : null, [policy]);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const entries = await listAuthorizedEntries();
      setAuthorizedEntries(entries);
      if (isAdmin) {
        const [loadedPolicy, loadedStatus] = await Promise.all([
          getPolicy(),
          getStatus(),
        ]);
        setPolicy(loadedPolicy);
        setStatus(loadedStatus);
      } else {
        setPolicy(null);
        setStatus(null);
      }
    } catch {
      setPolicy(null);
      setStatus(null);
      setAuthorizedEntries([]);
      showError("Your Vault list is unavailable.");
    } finally {
      setLoading(false);
    }
  }, [getPolicy, getStatus, isAdmin, listAuthorizedEntries]);

  useEffect(() => { void refresh(); }, [refresh]);

  useEffect(() => {
    const refreshOnFocus = () => { void refresh(); };
    window.addEventListener("focus", refreshOnFocus);
    return () => window.removeEventListener("focus", refreshOnFocus);
  }, [refresh]);

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

  const recordMountResult = (result: VaultMountEntryResult) => {
    setMountResults(current => ({ ...current, [result.entry_id]: result }));
  };

  const closeMountPrompt = () => {
    if (passwordInputRef.current) passwordInputRef.current.value = "";
    setMountTargetId(null);
  };

  const mountSelectedEntry = async () => {
    const entryId = mountTargetId;
    const input = passwordInputRef.current;
    let password = input?.value ?? "";
    if (!entryId || !password) return void showError("Enter the vault password to mount it.");

    // Clear the DOM field before awaiting IPC. The local stays only for this request.
    if (input) input.value = "";
    setMountTargetId(null);
    setMountingEntryId(entryId);
    try {
      const mountRequest = mountEntry(entryId, password);
      password = "";
      const result = await mountRequest;
      recordMountResult(result);
      if (result.state === "mounted") showSuccess(vaultMountResultLabel(result));
      else showError(vaultMountResultLabel(result));
      await refresh();
    } catch {
      showError("The Vault mount request could not be completed.");
    } finally {
      password = "";
      setMountingEntryId(null);
    }
  };

  const unmountSelectedEntry = async (entryId: string) => {
    setUnmountingEntryId(entryId);
    try {
      const result = await unmountEntry(entryId);
      recordMountResult(result);
      if (result.state === "unmounted") showSuccess(vaultMountResultLabel(result));
      else showError(vaultMountResultLabel(result));
      await refresh();
    } catch {
      showError("The Vault unmount request could not be completed.");
    } finally {
      setUnmountingEntryId(null);
    }
  };

  if (loading) return <div className="fleet-admin-stack">Loading your Vault access…</div>;
  const activePolicy = policy;
  const statusById = new Map(status?.entries.map(entry => [entry.id, entry]));
  const mountTarget = authorizedEntries.find(entry => entry.entry_id === mountTargetId)
    ?? activePolicy?.entries.find(entry => entry.id === mountTargetId);

  return (
    <div className="fleet-admin-stack">
      <Card>
        <CardHeader>
          <CardTitle>My vaults</CardTitle>
          <CardDescription>
            Only Vaults that the service has authorized for this Windows account appear here.
          </CardDescription>
        </CardHeader>
        <CardContent className="fleet-admin-stack">
          {authorizedEntries.length === 0 && (
            <p className="fleet-field-hint">No Vault access is currently assigned to this Windows account.</p>
          )}
          {authorizedEntries.map(entry => {
            const mountResult = mountResults[entry.entry_id];
            const isMounted = mountResult?.state === "mounted" || entry.mount_state === "mounted";
            return <div className="fleet-vault-lifecycle" key={entry.entry_id}>
              <div>
                <strong>{entry.label}</strong>
                <span className="fleet-vault-access-label">{vaultPresentationLabel(entry.presentation)} · {entry.access === "write" ? "Read & write" : "Read only"}</span>
                <p className="fleet-field-hint">
                  {mountResult
                    ? vaultMountResultLabel(mountResult)
                    : entry.drive_letter
                      ? `Mounted at ${entry.drive_letter}`
                      : entry.mount_state === "mounted"
                        ? "Mounted for this Windows session"
                        : "Ready to mount when needed"}
                </p>
              </div>
              {isMounted ? (
                <Button variant="outline" size="sm" disabled={unmountingEntryId === entry.entry_id} onClick={() => void unmountSelectedEntry(entry.entry_id)}>
                  {unmountingEntryId === entry.entry_id ? "Unmounting…" : "Unmount"}
                </Button>
              ) : (
                <Button variant="primary" size="sm" disabled={mountingEntryId === entry.entry_id} onClick={() => setMountTargetId(entry.entry_id)}>
                  {mountingEntryId === entry.entry_id ? "Mounting…" : "Mount"}
                </Button>
              )}
            </div>;
          })}
          <Button variant="outline" size="sm" onClick={() => void refresh()}>Refresh Vault access</Button>
        </CardContent>
      </Card>

      {isAdmin && <>
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
            const mountResult = mountResults[entry.id];
            const isMounted = mountResult?.state === "mounted" || observed?.mount_state === "mounted";
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
              <div className="fleet-vault-lifecycle">
                <div>
                  <strong>{vaultPresentationLabel(entry.mount.presentation)}</strong>
                  <p className="fleet-field-hint">
                    {mountResult
                      ? vaultMountResultLabel(mountResult)
                      : observed?.mount_state
                        ? `Service state: ${observed.mount_state}`
                        : observedResult(observed)}
                  </p>
                </div>
                {isMounted ? (
                  <Button variant="outline" size="sm" disabled={unmountingEntryId === entry.id} onClick={() => void unmountSelectedEntry(entry.id)}>
                    {unmountingEntryId === entry.id ? "Unmounting…" : "Unmount"}
                  </Button>
                ) : (
                  <Button variant="primary" size="sm" disabled={mountingEntryId === entry.id} onClick={() => setMountTargetId(entry.id)}>
                    {mountingEntryId === entry.id ? "Mounting…" : "Mount"}
                  </Button>
                )}
              </div>
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
            {status?.entries.map((entry, index) => <li key={entry.id}>Vault {index + 1}: {observedResult(entry)}</li>)}
          </ul>
        </CardContent>
      </Card>
      </>}

      <Dialog open={mountTargetId !== null} onOpenChange={open => { if (!open) closeMountPrompt(); }}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>Mount {mountTarget?.label ?? "Vault"}</DialogTitle>
            <DialogDescription>
              Enter the password for this mount only. It is cleared before the mount request finishes and is never saved.
            </DialogDescription>
          </DialogHeader>
          <Input
            ref={passwordInputRef}
            aria-label="Vault password"
            type="password"
            autoComplete="off"
            onKeyDown={event => {
              if (event.key === "Enter") void mountSelectedEntry();
            }}
          />
          <DialogFooter>
            <Button variant="outline" onClick={closeMountPrompt}>Cancel</Button>
            <Button variant="primary" onClick={() => void mountSelectedEntry()}>Mount Vault</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
