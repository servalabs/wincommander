import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Icon } from "@/components/ui/icon";
import { Input } from "@/components/ui/input";
import useVaultAccess from "@/hooks/useVaultAccess";
import { showError, showSuccess } from "@/utils/toast";
import {
  clearVaultAccessDraft, readVaultAccessDraftSnapshot, rebaseVaultAccessDraft, writeVaultAccessDraft,
} from "./vaultAccessDraft";
import { readUntrustedLegacyVaultDraft } from "./vaultLegacyImport";
import type { FleetAccessDirectory } from "./accessControlTypes";
import {
  newVaultEntry, newVaultPolicy, normalizeVaultAccessPolicy, validateVaultAccessIntent, vaultMountResultLabel, vaultPresentationLabel,
  type VaultAuthorizedEntry,
  type VaultMountEntryResult,
  type VaultAccessEntry, type VaultAccessPolicy, type VaultPolicyStatus, type VaultVolumeKind, type VaultVolumeRole,
} from "./vaultAccessTypes";
import { applyVaultAccessPreset, VAULT_ACCESS_PRESETS, vaultAccessPreset, type VaultAccessPreset } from "./vaultAccessPresets";
import VaultAccessPatternPicker from "./VaultAccessPatternPicker";
import { vaultPolicyVerification } from "./vaultAccessPresentation";
import { patchAuthorizedEntriesFromMountResult } from "./vaultAccessUiState";

function appliedAt(timestamp: number) {
  return new Date(timestamp * 1000).toLocaleString();
}

interface MountTarget {
  entryId: string;
  volumeKind: VaultVolumeKind;
}

export default function VaultAccessTab({ isAdmin, directory }: { isAdmin: boolean; directory: FleetAccessDirectory }) {
  const initialDraft = useMemo(() => readVaultAccessDraftSnapshot(), []);
  const [policy, setPolicy] = useState<VaultAccessPolicy | null>(initialDraft?.policy ?? null);
  const [status, setStatus] = useState<VaultPolicyStatus | null>(null);
  const [authorizedEntries, setAuthorizedEntries] = useState<VaultAuthorizedEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [legacyNotice, setLegacyNotice] = useState<string | null>(null);
  const [mountTarget, setMountTarget] = useState<MountTarget | null>(null);
  const [volumeRole, setVolumeRole] = useState<VaultVolumeRole>("outer");
  const [draftConfirmation, setDraftConfirmation] = useState<"replace" | "discard" | null>(null);
  const [mountingEntryId, setMountingEntryId] = useState<string | null>(null);
  const [unmountingEntryId, setUnmountingEntryId] = useState<string | null>(null);
  const [mountResults, setMountResults] = useState<Record<string, VaultMountEntryResult>>({});
  const passwordInputRef = useRef<HTMLInputElement>(null);
  const hiddenProtectionPasswordInputRef = useRef<HTMLInputElement>(null);
  const policyRef = useRef<VaultAccessPolicy | null>(initialDraft?.policy ?? null);
  const draftBaseRef = useRef<VaultAccessPolicy | null>(initialDraft?.basePolicy ?? null);
  const [savedPolicy, setSavedPolicy] = useState<VaultAccessPolicy | null>(null);
  const dirtyRef = useRef(initialDraft !== null);
  const draftWriteTimer = useRef<number | null>(null);
  const focusRefreshTimer = useRef<number | null>(null);
  const lastRefreshErrorAt = useRef(0);
  const [draftDirty, setDraftDirty] = useState(initialDraft !== null);
  const { getPolicy, getStatus, applyPolicy, mountEntry, unmountEntry, listAuthorizedEntries } = useVaultAccess<VaultAccessPolicy, VaultPolicyStatus>();
  const error = useMemo(() => policy ? validateVaultAccessIntent(policy) : null, [policy]);

  const replacePolicy = useCallback((next: VaultAccessPolicy | null, dirty: boolean, basePolicy?: VaultAccessPolicy | null) => {
    policyRef.current = next;
    dirtyRef.current = dirty;
    if (basePolicy !== undefined) draftBaseRef.current = basePolicy;
    if (!dirty) draftBaseRef.current = next;
    setPolicy(next);
    setDraftDirty(dirty);
    if (draftWriteTimer.current !== null) window.clearTimeout(draftWriteTimer.current);
    if (dirty && next) {
      // Persist the recovery draft, not each individual keystroke.
      draftWriteTimer.current = window.setTimeout(() => writeVaultAccessDraft(next, undefined, draftBaseRef.current), 300);
    } else clearVaultAccessDraft();
  }, []);

  const editPolicy = useCallback((update: (current: VaultAccessPolicy | null) => VaultAccessPolicy | null) => {
    replacePolicy(update(policyRef.current), true);
  }, [replacePolicy]);

  const refresh = useCallback(async (replaceDirtyDraft = false, includeStatus = true) => {
    try {
      if (isAdmin) {
        const [entries, loadedPolicy, loadedStatus] = await Promise.all([
          listAuthorizedEntries(),
          getPolicy().then(normalizeVaultAccessPolicy),
          includeStatus ? getStatus() : Promise.resolve(null),
        ]);
        setAuthorizedEntries(entries);
        setSavedPolicy(loadedPolicy);
        if (replaceDirtyDraft || !dirtyRef.current) replacePolicy(loadedPolicy, false);
        else if (!draftBaseRef.current && policyRef.current?.version === loadedPolicy.version) draftBaseRef.current = loadedPolicy;
        if (loadedStatus) setStatus(loadedStatus);
      } else {
        setAuthorizedEntries(await listAuthorizedEntries());
        setStatus(null);
      }
    } catch {
      // A service/status refresh must never destroy an administrator's draft.
      const now = Date.now();
      if (now - lastRefreshErrorAt.current > 30_000) {
        lastRefreshErrorAt.current = now;
        showError("Your Vault list is unavailable.");
      }
    } finally {
      setLoading(false);
    }
  }, [getPolicy, getStatus, isAdmin, listAuthorizedEntries, replacePolicy]);

  useEffect(() => { void refresh(); }, [refresh]);

  useEffect(() => {
    const refreshOnFocus = () => {
      if (dirtyRef.current || focusRefreshTimer.current !== null) return;
      focusRefreshTimer.current = window.setTimeout(() => {
        focusRefreshTimer.current = null;
        if (!dirtyRef.current) void refresh();
      }, 500);
    };
    window.addEventListener("focus", refreshOnFocus);
    return () => {
      window.removeEventListener("focus", refreshOnFocus);
      if (focusRefreshTimer.current !== null) {
        window.clearTimeout(focusRefreshTimer.current);
        focusRefreshTimer.current = null;
      }
    };
  }, [refresh]);

  useEffect(() => () => {
    if (draftWriteTimer.current !== null) window.clearTimeout(draftWriteTimer.current);
  }, []);

  const updateEntry = (id: string, patch: Partial<VaultAccessEntry>) => editPolicy(current => {
    const source = current ?? newVaultPolicy();
    return { ...source, entries: source.entries.map(entry => entry.id === id ? { ...entry, ...patch } : entry) };
  });

  const setAccessPreset = (id: string, preset: Exclude<VaultAccessPreset, "custom">) => editPolicy(current => {
    const source = current ?? newVaultPolicy();
    return {
      ...source,
      entries: source.entries.map(entry => entry.id === id ? applyVaultAccessPreset(entry, preset) : entry),
    };
  });

  const setOwnerAccount = (id: string, ownerAccount: string) => editPolicy(current => {
    const source = current ?? newVaultPolicy();
    return {
      ...source,
      entries: source.entries.map(entry => {
        if (entry.id !== id) return entry;
        const ownerChanged = { ...entry, owner_account: ownerAccount };
        return vaultAccessPreset(entry) === "private"
          ? applyVaultAccessPreset(ownerChanged, "private")
          : ownerChanged;
      }),
    };
  });

  const apply = async () => {
    if (!policy) return;
    if (error) return void showError(error);
    setSaving(true);
    try {
      const submittedPolicy = {
        // The service's optimistic lock accepts only the next revision. The
        // displayed version remains the last observed policy until refresh.
        ...policy,
        expected_previous_version: policy.version,
        version: policy.version + 1,
      };
      const appliedStatus = await applyPolicy(submittedPolicy);
      replacePolicy(submittedPolicy, false);
      setStatus(appliedStatus);
      // Applying a policy can change this caller's authorized rows, but the
      // returned status is already current; avoid an immediate duplicate read.
      await refresh(true, false);
      if (appliedStatus.validation_state === "degraded") {
        showError("Vault settings were saved with warnings. Fix the listed access problems before mounting.");
      } else {
        showSuccess("Vault settings saved. Future mounts only need the password.");
      }
    } catch (cause) {
      showError(cause instanceof Error ? cause.message : "Vault policy was not applied.");
    } finally {
      setSaving(false);
    }
  };

  const importLegacyDraft = () => {
    const imported = readUntrustedLegacyVaultDraft(directory);
    if (!imported) return void setLegacyNotice("No retired local planner draft was found.");
    if (imported.entries.length === 0) {
      return void setLegacyNotice(`No safe planner entries were imported. ${imported.skippedVolumeCount} vault${imported.skippedVolumeCount === 1 ? " was" : "s were"} skipped because the owner could not be resolved.`);
    }
    editPolicy(current => ({ ...(current ?? newVaultPolicy()), entries: imported.entries }));
    const drops = imported.droppedPrincipalCount > 0 ? ` ${imported.droppedPrincipalCount} unresolved grant${imported.droppedPrincipalCount === 1 ? " was" : "s were"} dropped.` : "";
    const skips = imported.skippedVolumeCount > 0 ? ` ${imported.skippedVolumeCount} vault${imported.skippedVolumeCount === 1 ? " was" : "s were"} skipped.` : "";
    setLegacyNotice(`Imported as an untrusted draft. Review container paths and grants before applying.${drops}${skips}`);
  };

  const replaceWithSharedDraft = () => {
    const source = newVaultPolicy();
    const entry = newVaultEntry("shared");
    entry.owner_account = "";
    entry.grants = [{ principal_name: "", access: "write" }];
    replacePolicy({ ...source, entries: [entry] }, true);
  };

  const createSharedDraft = () => {
    if (policyRef.current) return void setDraftConfirmation("replace");
    replaceWithSharedDraft();
  };

  const reloadSavedPolicy = async () => {
    try {
      const loaded = normalizeVaultAccessPolicy(await getPolicy());
      replacePolicy(loaded, false);
      setSavedPolicy(loaded);
      await refresh(true);
    } catch {
      showError("Saved Vault settings could not be reloaded. Your draft is still available.");
    }
  };

  const rebaseDraft = async () => {
    const draft = policyRef.current;
    if (!draft) return;
    try {
      const latest = savedPolicy ?? normalizeVaultAccessPolicy(await getPolicy());
      setSavedPolicy(latest);
      const rebased = rebaseVaultAccessDraft(draft, draftBaseRef.current, latest);
      if (!rebased) return void showError("The saved policy changed in the same Vault. Your draft was kept; reload and review before editing again.");
      replacePolicy(rebased, true, latest);
      showSuccess("Draft rebased on the latest saved Vault settings.");
    } catch {
      showError("Latest Vault settings could not be loaded. Your draft is still available.");
    }
  };

  const discardDraftAndReload = () => {
    if (draftDirty) return void setDraftConfirmation("discard");
    void reloadSavedPolicy();
  };

  const confirmDraftChange = () => {
    const action = draftConfirmation;
    setDraftConfirmation(null);
    if (action === "replace") replaceWithSharedDraft();
    if (action === "discard") void reloadSavedPolicy();
  };

  const recordMountResult = (result: VaultMountEntryResult) => {
    setMountResults(current => ({ ...current, [result.entry_id]: result }));
    setAuthorizedEntries(current => patchAuthorizedEntriesFromMountResult(current, result));
  };

  const closeMountPrompt = () => {
    if (passwordInputRef.current) passwordInputRef.current.value = "";
    if (hiddenProtectionPasswordInputRef.current) hiddenProtectionPasswordInputRef.current.value = "";
    setMountTarget(null);
    setVolumeRole("outer");
  };

  const openMountPrompt = (entry: { entry_id?: string; id?: string; volume_kind: VaultVolumeKind }) => {
    const entryId = entry.entry_id ?? entry.id;
    if (!entryId) return;
    setVolumeRole("outer");
    setMountTarget({ entryId, volumeKind: entry.volume_kind ?? "standard" });
  };

  const mountSelectedEntry = async () => {
    const entryId = mountTarget?.entryId;
    const requestedRole = mountTarget?.volumeKind === "dual" ? volumeRole : "outer";
    const input = passwordInputRef.current;
    const hiddenProtectionInput = hiddenProtectionPasswordInputRef.current;
    let password = input?.value ?? "";
    let hiddenProtectionPassword = requestedRole === "outer" ? hiddenProtectionInput?.value ?? "" : "";
    if (!entryId || !password) return void showError("Enter the vault password to mount it.");

    // Clear the DOM field before awaiting IPC. The local stays only for this request.
    if (input) input.value = "";
    if (hiddenProtectionInput) hiddenProtectionInput.value = "";
    setMountTarget(null);
    setMountingEntryId(entryId);
    try {
      const mountRequest = mountEntry(entryId, password, requestedRole, hiddenProtectionPassword || undefined);
      password = "";
      hiddenProtectionPassword = "";
      const result = await mountRequest;
      recordMountResult(result);
      if (result.state === "mounted") showSuccess(vaultMountResultLabel(result));
      else showError(vaultMountResultLabel(result));
    } catch {
      showError("The Vault mount request could not be completed.");
    } finally {
      password = "";
      hiddenProtectionPassword = "";
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
    } catch {
      showError("The Vault unmount request could not be completed.");
    } finally {
      setUnmountingEntryId(null);
    }
  };

  if (loading) return <div className="fleet-admin-stack">Loading your Vault access…</div>;
  const activePolicy = policy;
  const authorizedById = new Map(authorizedEntries.map(entry => [entry.entry_id, entry]));
  const verification = draftDirty ? null : vaultPolicyVerification(status);
  const mountTargetEntry = authorizedEntries.find(entry => entry.entry_id === mountTarget?.entryId)
    ?? activePolicy?.entries.find(entry => entry.id === mountTarget?.entryId);

  return (
    <div className="fleet-admin-stack">
      <Card>
        <CardHeader>
          <CardTitle>{isAdmin ? "Saved vaults" : "My vaults"}</CardTitle>
          <CardDescription>
            {isAdmin
              ? "Saved vault settings stay on this PC. Mounting asks only for the password, which is never stored."
              : "Only Vaults that the service has authorized for this Windows account appear here; mounting asks only for the password."}
          </CardDescription>
        </CardHeader>
        <CardContent className="fleet-admin-stack">
          {authorizedEntries.length === 0 && (
            <p className="fleet-field-hint">{isAdmin ? "No vault has been saved and assigned to this account yet." : "No Vault access is currently assigned to this Windows account."}</p>
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
                <Button variant="primary" size="sm" disabled={mountingEntryId === entry.entry_id} onClick={() => openMountPrompt(entry)}>
                  {mountingEntryId === entry.entry_id ? "Mounting…" : "Mount"}
                </Button>
              )}
            </div>;
          })}
          <div className="fleet-vault-refresh-row">
            <Button variant="outline" size="sm" onClick={() => void refresh()}><Icon icon="refresh" size={14} />Refresh</Button>
          </div>
        </CardContent>
      </Card>

      {isAdmin && <>
      <Card>
        <CardHeader>
          <CardTitle>Vault access</CardTitle>
          <CardDescription>Set the vault details, choose an everyday access pattern, and add the Windows users or groups who need it. WinCommander verifies the names and applies the matching security settings.</CardDescription>
        </CardHeader>
        <CardContent className="fleet-admin-stack">
          {!activePolicy && <div className="fleet-vault-empty-setup">
            <div><strong>No vault access is configured yet</strong><span>Start with one shared vault, or use the recommended personal-and-shared starter.</span></div>
            <div className="fleet-action-row">
              <Button onClick={createSharedDraft}>Create first shared vault</Button>
              <Button variant="outline" onClick={() => editPolicy(current => current ?? newVaultPolicy())}>Use three-vault starter</Button>
            </div>
          </div>}
          {activePolicy && <p className="fleet-field-hint">{draftDirty ? "Draft auto-saved on this PC — not yet applied to Windows." : "Showing the policy saved by the security service."}</p>}
          {activePolicy?.entries.map((entry, entryIndex) => {
            const authorized = authorizedById.get(entry.id);
            const mountResult = mountResults[entry.id];
            const isMounted = mountResult?.state === "mounted" || authorized?.mount_state === "mounted";
            const accessPreset = vaultAccessPreset(entry);
            const isDualContainer = entry.volume_kind === "dual";
            return <div className="fleet-vault-workspace" key={entry.id}>
              <div className="fleet-vault-workspace-header">
                <div><span className="fleet-vault-step">1. Vault details</span><strong>Vault {entryIndex + 1}</strong></div>
                <Button variant="outline" size="sm" onClick={() => editPolicy(current => current && ({ ...current, entries: current.entries.filter(item => item.id !== entry.id) }))}>Remove</Button>
              </div>
              <div className="fleet-owner-inputs">
                <label className="fleet-field"><span>Vault name</span><Input aria-label={`Vault ${entryIndex + 1} label`} value={entry.label} placeholder="Shared vault" onChange={event => updateEntry(entry.id, { label: event.target.value })} /><small>The label people recognize.</small></label>
                <label className="fleet-field"><span>Container file</span><Input aria-label={`Vault ${entryIndex + 1} container path`} value={entry.container_path} placeholder="D:\\Vaults\\shared.hc" onChange={event => updateEntry(entry.id, { container_path: event.target.value })} /><small>The encrypted .hc file on this PC. Keep each managed Vault in its own dedicated parent folder.</small></label>
                <label className="fleet-field"><span>Container type</span><select aria-label={`Vault ${entryIndex + 1} container type`} value={entry.volume_kind} onChange={event => updateEntry(entry.id, event.target.value === "dual" ? { ...applyVaultAccessPreset(entry, "private"), volume_kind: "dual" } : { volume_kind: "standard" })}><option value="standard">Standard container</option><option value="dual">Outer + hidden container</option></select><small>{isDualContainer ? "Outer and hidden passwords are requested only when mounting; this Vault stays private to its owner." : "Standard containers can be private or shared."}</small></label>
                <label className="fleet-field"><span>Primary owner</span><Input aria-label={`Vault ${entryIndex + 1} owner`} value={entry.owner_account} placeholder="SERVER\\shrey" onChange={event => setOwnerAccount(entry.id, event.target.value)} /><small>The Windows account responsible for this Vault. Use PC-or-domain\username.</small></label>
                <label className="fleet-field"><span>Drive letter</span><Input aria-label={`Vault ${entryIndex + 1} preferred drive letter`} value={entry.mount.preferred_letter ?? ""} maxLength={1} placeholder="V" onChange={event => updateEntry(entry.id, { mount: { ...entry.mount, preferred_letter: event.target.value.toUpperCase() || undefined } })} /><small>The preferred letter in File Explorer. Leave blank for Windows to choose.</small></label>
              </div>
              {isDualContainer
                ? <p className="fleet-field-hint">2. Dual containers are owner-only. They cannot be shared through Fleet access policy.</p>
                : <VaultAccessPatternPicker value={accessPreset} onChange={preset => setAccessPreset(entry.id, preset)} />}
              <div className="fleet-vault-grants">
                <strong>{accessPreset === "private" || isDualContainer ? "3. Confirm owner access" : "3. Add Windows users or groups"}</strong>
                {!isDualContainer && accessPreset !== "private" && <p className="fleet-field-hint">A Windows user or group gives that account or team the access selected above.</p>}
                {accessPreset === "private" || isDualContainer
                  ? <p className="fleet-field-hint">Only the primary owner can mount or edit this vault. Its drive appears only in that Windows session.</p>
                  : entry.grants.map((grant, grantIndex) => <div className="fleet-vault-grant-row" key={`${entry.id}-${grantIndex}`}>
                    <label className="fleet-field"><span>Windows user or group</span><Input aria-label={`Grant ${grantIndex + 1} principal`} value={grant.principal_name} placeholder="SERVER\\Admins" onChange={event => updateEntry(entry.id, { grants: entry.grants.map((current, index) => index === grantIndex ? { ...current, principal_name: event.target.value } : current) })} /><small>{accessPreset === "custom" ? "This person has the level selected beside them." : VAULT_ACCESS_PRESETS[accessPreset].label}</small></label>
                    {accessPreset === "custom" && <label className="fleet-field"><span>Access</span><select aria-label={`Grant ${grantIndex + 1} access`} value={grant.access} onChange={event => updateEntry(entry.id, { grants: entry.grants.map((current, index) => index === grantIndex ? { ...current, access: event.target.value as "read" | "write" } : current) })}><option value="write">Can edit</option><option value="read">View only</option></select></label>}
                    <Button variant="outline" size="sm" onClick={() => updateEntry(entry.id, { grants: entry.grants.filter((_, index) => index !== grantIndex) })}>Remove</Button>
                  </div>)}
                {!isDualContainer && accessPreset !== "private" && <Button className="fleet-vault-add-grant" variant="outline" size="sm" onClick={() => updateEntry(entry.id, { grants: [...entry.grants, { principal_name: "", access: entry.grants[0]?.access ?? "write" }] })}>Add person or group</Button>}
              </div>
              <div className="fleet-vault-lifecycle">
                <div>
                  <strong>{vaultPresentationLabel(entry.mount.presentation)}</strong>
                  <p className="fleet-field-hint">
                    {mountResult
                      ? vaultMountResultLabel(mountResult)
                      : authorized?.drive_letter
                        ? `Mounted at ${authorized.drive_letter}`
                        : authorized?.mount_state === "mounted"
                          ? "Mounted for this Windows session"
                          : authorized
                            ? "Ready to mount when needed"
                            : status?.validation_state === "degraded"
                              ? "Mounting is unavailable until the saved access settings are fixed."
                              : "Mount access is checked for the signed-in Windows account."}
                  </p>
                </div>
                {isMounted ? (
                  <Button variant="outline" size="sm" disabled={unmountingEntryId === entry.id} onClick={() => void unmountSelectedEntry(entry.id)}>
                    {unmountingEntryId === entry.id ? "Unmounting…" : "Unmount"}
                  </Button>
                ) : (
                  <Button variant="primary" size="sm" disabled={mountingEntryId === entry.id} onClick={() => openMountPrompt(entry)}>
                    {mountingEntryId === entry.id ? "Mounting…" : "Mount"}
                  </Button>
                )}
              </div>
            </div>;
          })}
          <div className="fleet-action-row">
            <Button variant="outline" onClick={() => editPolicy(current => {
              const source = current ?? newVaultPolicy();
              return { ...source, entries: [...source.entries, newVaultEntry()] };
            })}>Add private vault</Button>
            <Button variant="outline" onClick={() => editPolicy(current => {
              const source = current ?? newVaultPolicy();
              return { ...source, entries: [...source.entries, newVaultEntry("shared")] };
            })}>Add shared vault</Button>
            <Button variant="primary" disabled={saving || !!error} onClick={() => void apply()}>{saving ? "Saving…" : "Save vault settings"}</Button>
            {verification?.tone === "success" && <span className="fleet-vault-save-status" role="status"><Icon icon="tick-circle" size={14} />{verification.title}{verification.appliedAt != null ? ` · ${appliedAt(verification.appliedAt)}` : ""}</span>}
          </div>
          {error && <p className="fleet-validation-errors">{error}</p>}
          {verification?.tone === "warning" && <div className="fleet-vault-verification-warning" role="alert">
            <Icon icon="warning-sign" size={16} />
            <div><strong>{verification.title}</strong><p>{verification.detail}</p></div>
          </div>}
          <details className="fleet-vault-advanced">
            <summary>Advanced and recovery</summary>
            <p>Use these only to import an older planner draft or discard local edits and return to the last settings saved by Windows.</p>
            <div className="fleet-action-row">
              <Button variant="outline" size="sm" onClick={importLegacyDraft}>Import retired planner as draft</Button>
              {draftDirty && <Button variant="outline" size="sm" onClick={() => void rebaseDraft()}>Rebase draft with saved settings</Button>}
              <Button variant="outline" size="sm" onClick={() => void discardDraftAndReload()}>Discard draft & reload saved</Button>
            </div>
            {legacyNotice && <span className="fleet-field-hint">{legacyNotice}</span>}
          </details>
        </CardContent>
      </Card>

      </>}

      <Dialog open={mountTarget !== null} onOpenChange={open => { if (!open) closeMountPrompt(); }}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>Mount {mountTargetEntry?.label ?? "Vault"}</DialogTitle>
            <DialogDescription>
              Enter the password for this mount only. It is cleared before the mount request finishes and is never saved.
            </DialogDescription>
          </DialogHeader>
          {mountTarget?.volumeKind === "dual" && <label className="fleet-field"><span>Open</span><select aria-label="Vault volume role" value={volumeRole} onChange={event => setVolumeRole(event.target.value as VaultVolumeRole)}><option value="outer">Outer volume</option><option value="hidden">Hidden volume</option></select><small>Choose the volume for this mount only. The choice and password are never saved.</small></label>}
          {mountTarget?.volumeKind === "dual" && volumeRole === "outer" && <Input ref={hiddenProtectionPasswordInputRef} aria-label="Hidden volume protection password" type="password" autoComplete="off" placeholder="Hidden password to protect it while writing (optional)" />}
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

      <Dialog open={draftConfirmation !== null} onOpenChange={open => { if (!open) setDraftConfirmation(null); }}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>{draftConfirmation === "replace" ? "Replace this draft?" : "Discard local changes?"}</DialogTitle>
            <DialogDescription>
              {draftConfirmation === "replace"
                ? "This replaces the fields currently in the editor with one blank shared vault."
                : "This removes the local draft and reloads the last policy saved by the security service."}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDraftConfirmation(null)}>Keep editing</Button>
            <Button variant="primary" onClick={confirmDraftChange}>{draftConfirmation === "replace" ? "Replace draft" : "Discard & reload"}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
