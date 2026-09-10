import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Icon } from "@/components/ui/icon";
import { Input } from "@/components/ui/input";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import useVaultAccess from "@/hooks/useVaultAccess";
import { showError, showSuccess } from "@/utils/toast";
import { newDiagnosticOperationId, recordDiagnostic } from "@/lib/diagnostics";
import {
  clearVaultAccessDraft, readVaultAccessDraftSnapshot, rebaseVaultAccessDraft, writeVaultAccessDraft,
} from "./vaultAccessDraft";
import { readUntrustedLegacyVaultDraft } from "./vaultLegacyImport";
import type { FleetAccessDirectory } from "./accessControlTypes";
import {
  newVaultEntry, newVaultPolicy, nextVaultAccessPolicy, normalizeVaultAccessPolicy, removeVaultEntryDraft, validateVaultAccessIntent, vaultMountResultLabel, vaultPresentationLabel,
  type VaultAuthorizedEntry,
  type VaultMountEntryResult,
  type VaultAccess, type VaultAccessEntry, type VaultAccessPolicy, type VaultPolicyStatus, type VaultContainerKind, type VaultVolumeRole,
} from "./vaultAccessTypes";
import { applyVaultAccessPreset, vaultAccessPreset, type VaultAccessPreset } from "./vaultAccessPresets";
import VaultAccessEditor from "./VaultAccessEditor";
import { vaultEntryResultLabel, vaultPolicyVerification } from "./vaultAccessPresentation";
import { patchAuthorizedEntriesFromMountResult, vaultMountGate } from "./vaultAccessUiState";

function appliedAt(timestamp: number) {
  return new Date(timestamp * 1000).toLocaleString();
}

const VAULT_LIST_RETRY_DELAY_MS = 300;

function vaultListFailure(cause: unknown): { category: "service_connect" | "service_reply" | "service_denied" | "unknown"; message: string } {
  // Do not put a backend error into the renderer or diagnostic store: a list
  // request can fail while the service is restarting, and the original error
  // may contain OS transport detail.  Keep the user-facing result actionable
  // but bounded instead.
  const detail = cause instanceof Error ? cause.message.toLowerCase() : "";
  if (detail.includes("rejected request") || detail.includes("forbidden")) {
    return {
      category: "service_denied",
      message: "The local Vault service denied this list request. Close every WinCommander window and open it again.",
    };
  }
  if (detail.includes("reply") || detail.includes("signature") || detail.includes("unexpected")) {
    return {
      category: "service_reply",
      message: "The local Vault service did not complete its secure reply. Refresh WinCommander after the service has finished starting.",
    };
  }
  if (detail.includes("connect") || detail.includes("hello") || detail.includes("pipe") || detail.includes("timed out")) {
    return {
      category: "service_connect",
      message: "WinCommander could not reach its local Vault service. It may still be starting; refresh in a moment.",
    };
  }
  return {
    category: "unknown",
    message: "WinCommander could not load the Vault list. Refresh the Vault page; if it repeats, use the reference below when contacting support.",
  };
}

function vaultPolicySaveFailure(cause: unknown): { code: "VLT.POLICY.ELEVATION_REQUIRED" | "VLT.POLICY.VERSION_CONFLICT" | "VLT.POLICY.APPLY_FAILED"; message: string } {
  // Keep the service's transport/Windows detail out of the UI.  The service
  // already makes the authorization decision; this only turns its fixed error
  // categories into an action the person can take.
  const detail = cause instanceof Error ? cause.message.toLowerCase() : "";
  if (detail.includes("forbidden") || detail.includes("privileged")) {
    return {
      code: "VLT.POLICY.ELEVATION_REQUIRED",
      message: "Vault settings require an elevated WinCommander window. Close this window, then start WinCommander with Run as administrator. Your assigned Vaults can still be mounted normally.",
    };
  }
  if (detail.includes("version conflict") || detail.includes("changed elsewhere")) {
    return {
      code: "VLT.POLICY.VERSION_CONFLICT",
      message: "Vault settings changed in another WinCommander window. Refresh this page before saving again.",
    };
  }
  return {
    code: "VLT.POLICY.APPLY_FAILED",
    message: "Vault settings could not be saved. Refresh the Vault page and retry; use the reference below if it repeats.",
  };
}

interface MountTarget {
  entryId: string;
  containerKind: VaultContainerKind;
  access?: VaultAccess;
}

export default function VaultAccessTab({ isAdmin, directory }: { isAdmin: boolean; directory: FleetAccessDirectory }) {
  const initialDraft = useMemo(() => readVaultAccessDraftSnapshot(), []);
  const [policy, setPolicy] = useState<VaultAccessPolicy | null>(initialDraft?.policy ?? null);
  const [status, setStatus] = useState<VaultPolicyStatus | null>(null);
  const [authorizedEntries, setAuthorizedEntries] = useState<VaultAuthorizedEntry[]>([]);
  const [canManagePolicy, setCanManagePolicy] = useState(false);
  const [policyLoadUnavailable, setPolicyLoadUnavailable] = useState(false);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [legacyNotice, setLegacyNotice] = useState<string | null>(null);
  const [mountTarget, setMountTarget] = useState<MountTarget | null>(null);
  const [volumeRole, setVolumeRole] = useState<VaultVolumeRole>("outer");
  const [draftConfirmation, setDraftConfirmation] = useState<"replace" | "discard" | null>(null);
  const [policyRemovalConfirmation, setPolicyRemovalConfirmation] = useState(false);
  const [existingVaultDialogOpen, setExistingVaultDialogOpen] = useState(false);
  const [existingVaultPath, setExistingVaultPath] = useState("");
  const [existingVaultLabel, setExistingVaultLabel] = useState("");
  const [selectedEntryId, setSelectedEntryId] = useState<string | null>(initialDraft?.policy?.entries[0]?.id ?? null);
  const [editorMode, setEditorMode] = useState<"details" | "access">("details");
  const [mountingEntryId, setMountingEntryId] = useState<string | null>(null);
  const [unmountingEntryId, setUnmountingEntryId] = useState<string | null>(null);
  const [mountResults, setMountResults] = useState<Record<string, VaultMountEntryResult>>({});
  const passwordInputRef = useRef<HTMLInputElement>(null);
  const hiddenProtectionPasswordInputRef = useRef<HTMLInputElement>(null);
  const editorRef = useRef<HTMLDivElement>(null);
  const policyRef = useRef<VaultAccessPolicy | null>(initialDraft?.policy ?? null);
  const draftBaseRef = useRef<VaultAccessPolicy | null>(initialDraft?.basePolicy ?? null);
  const dirtyRef = useRef(initialDraft !== null);
  const draftWriteTimer = useRef<number | null>(null);
  const focusRefreshTimer = useRef<number | null>(null);
  const lastRefreshErrorAt = useRef(0);
  const refreshRevision = useRef(0);
  const saveInProgress = useRef(false);
  const [draftDirty, setDraftDirty] = useState(initialDraft !== null);
  const { getPolicy, getStatus, applyPolicy, mountEntry, unmountEntry, listAuthorizedEntries, getCapabilities } = useVaultAccess<VaultAccessPolicy, VaultPolicyStatus>();
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
    const revision = ++refreshRevision.current;
    try {
      // Windows can report that an account belongs to Administrators while UAC
      // gives this app a standard token. Ask the service about this *process*
      // before making privileged policy calls; the ordinary authorised-vault
      // list must remain available either way.
      // A developer restart can launch the window a fraction before the
      // restarted SYSTEM service accepts its pipe.  Retry this read once;
      // access decisions remain entirely service-owned.
      const entries = await listAuthorizedEntries().catch(async () => {
        await new Promise<void>(resolve => window.setTimeout(resolve, VAULT_LIST_RETRY_DELAY_MS));
        return listAuthorizedEntries();
      });
      // A successful admin capability check must not be overwritten by a
      // transient policy/status read.  Those are different operations: an
      // unavailable policy read needs a retry message, not the misleading
      // "not elevated" warning.
      const capabilities = await getCapabilities().catch(async () => {
        await new Promise<void>(resolve => window.setTimeout(resolve, VAULT_LIST_RETRY_DELAY_MS));
        return getCapabilities();
      });
      if (revision !== refreshRevision.current) return false;
      setAuthorizedEntries(entries);
      setMountResults({});
      setCanManagePolicy(capabilities.can_manage_policy);
      setPolicyLoadUnavailable(false);
      if (capabilities.can_manage_policy) {
        try {
          const loadedPolicy = await getPolicy().then(value => value ? normalizeVaultAccessPolicy(value) : null);
          if (revision !== refreshRevision.current) return false;
          if (replaceDirtyDraft || !dirtyRef.current) replacePolicy(loadedPolicy, false);
          else if (!draftBaseRef.current && loadedPolicy && policyRef.current?.version === loadedPolicy.version) draftBaseRef.current = loadedPolicy;

          // Status is advisory.  A failure here must not hide an elevated
          // administrator's policy editor or turn into an elevation warning.
          if (includeStatus) {
            try {
              const loadedStatus = await getStatus();
              if (revision !== refreshRevision.current) return false;
              setStatus(loadedStatus);
            } catch {
              if (revision !== refreshRevision.current) return false;
              setStatus(null);
            }
          }
        } catch {
          // Keep the verified capability.  Rendering an editable replacement
          // policy after a failed read could overwrite real rules, so block
          // editing until the original policy can be loaded again.
          if (revision !== refreshRevision.current) return false;
          setPolicyLoadUnavailable(true);
          setStatus(null);
        }
      } else {
        setStatus(null);
      }
      return true;
    } catch (cause) {
      if (revision !== refreshRevision.current) return false;
      setAuthorizedEntries([]);
      setMountResults({});
      // A service/status refresh must never destroy an administrator's draft.
      const now = Date.now();
      if (now - lastRefreshErrorAt.current > 30_000) {
        lastRefreshErrorAt.current = now;
        const failure = vaultListFailure(cause);
        const operationId = newDiagnosticOperationId("vault");
        recordDiagnostic({ operationId, feature: "vault", action: "list_authorized", stage: "service_read", lifecycle: "verified", outcome: "failed", errorCode: "VLT.LIST.UNAVAILABLE", severity: "warn", retryability: "automatic", suggestedNextAction: "refresh_status", privacyClass: "local_sensitive", context: { reason_category: failure.category, retry_count: 1 } });
        showError(failure.message, undefined, { operationId });
      }
      return false;
    } finally {
      setLoading(false);
    }
  }, [getCapabilities, getPolicy, getStatus, listAuthorizedEntries, replacePolicy]);

  useEffect(() => { void refresh(); }, [refresh]);

  useEffect(() => {
    const refreshOnFocus = () => {
      if (saveInProgress.current || dirtyRef.current || focusRefreshTimer.current !== null) return;
      focusRefreshTimer.current = window.setTimeout(() => {
        focusRefreshTimer.current = null;
        if (!saveInProgress.current && !dirtyRef.current) void refresh();
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

  useEffect(() => {
    const entries = policy?.entries ?? [];
    if (entries.length === 0) {
      if (selectedEntryId !== null) setSelectedEntryId(null);
      return;
    }
    if (!selectedEntryId || !entries.some(entry => entry.id === selectedEntryId)) {
      setSelectedEntryId(entries[0]!.id);
    }
  }, [policy, selectedEntryId]);

  const updateEntry = (id: string, patch: Partial<VaultAccessEntry>) => editPolicy(current => {
    const source = current ?? newVaultPolicy();
    return { ...source, entries: source.entries.map(entry => entry.id === id ? { ...entry, ...patch } : entry) };
  });

  const openEntryEditor = (entryId: string, mode: "details" | "access") => {
    setSelectedEntryId(entryId);
    setEditorMode(mode);
    window.requestAnimationFrame(() => {
      editorRef.current?.scrollIntoView({ block: "nearest", behavior: "smooth" });
      const target = mode === "access"
        ? editorRef.current?.querySelector<HTMLElement>(".fleet-vault-grants select, .fleet-vault-grants button")
        : editorRef.current?.querySelector<HTMLElement>("input");
      target?.focus();
    });
  };

  const browseExistingVault = async () => {
    try {
      // Deliberately do not filter by filename extension. Existing VeraCrypt
      // containers can be valid without a conventional extension; the service
      // owns safe identity and accessibility validation when this draft saves.
      const selected = await openFileDialog({
        multiple: false,
        directory: false,
        title: "Select an existing encrypted Vault container",
      });
      if (typeof selected !== "string") return;
      setExistingVaultPath(selected);
      if (!existingVaultLabel.trim()) {
        setExistingVaultLabel(selected.replaceAll("/", "\\").split("\\").filter(Boolean).at(-1) ?? "Existing Vault");
      }
    } catch {
      showError("WinCommander could not open the file chooser. You can enter the container path instead.");
    }
  };

  const addExistingVault = () => {
    const containerPath = existingVaultPath.trim();
    if (!containerPath) return void showError("Choose or enter the encrypted container file first.");
    const entry = newVaultEntry("shared");
    entry.container_path = containerPath;
    entry.label = existingVaultLabel.trim()
      || containerPath.replaceAll("/", "\\").split("\\").filter(Boolean).at(-1)
      || "Existing Vault";
    editPolicy(current => {
      if (current) return { ...current, entries: [...current.entries, entry] };
      const next = newVaultPolicy();
      return { ...next, entries: [entry] };
    });
    setExistingVaultDialogOpen(false);
    setExistingVaultPath("");
    setExistingVaultLabel("");
    openEntryEditor(entry.id, "details");
  };

  const addVaultEntryDraft = (kind: "shared" | "private") => {
    const entry = newVaultEntry(kind);
    editPolicy(current => {
      const source = current ?? newVaultPolicy();
      return { ...source, entries: [...source.entries, entry] };
    });
    openEntryEditor(entry.id, "details");
  };

  const removeEntry = (id: string) => {
    const current = policyRef.current;
    if (!current) return;
    const next = removeVaultEntryDraft(current, id, draftBaseRef.current !== null);
    // A never-saved starter has no service policy to decommission. Removing
    // its final row is therefore a local discard, not an invalid version-0
    // deletion request.
    if (!next) {
      replacePolicy(null, false);
      setStatus(null);
      return;
    }
    replacePolicy(next, true);
  };

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
    if (saveInProgress.current) return;
    if (!policy) return;
    if (error) return void showError(error);
    saveInProgress.current = true;
    const operationId = newDiagnosticOperationId("vault");
    recordDiagnostic({ operationId, feature: "vault", action: "apply_policy", stage: "requested", lifecycle: "requested", outcome: "started", severity: "info", retryability: "never", suggestedNextAction: "none", privacyClass: "local_sensitive" });
    setSaving(true);
    ++refreshRevision.current;
    setAuthorizedEntries([]);
    setMountResults({});
    closeMountPrompt();
    try {
      // The service's optimistic lock accepts only the next revision. The
      // displayed version remains the last observed policy until refresh.
      const submittedPolicy = nextVaultAccessPolicy(policy);
      const appliedStatus = await applyPolicy(submittedPolicy);
      const removed = submittedPolicy.entries.length === 0;
      replacePolicy(removed ? null : submittedPolicy, false);
      setStatus(appliedStatus);
      if (removed) {
        setAuthorizedEntries([]);
        setMountResults({});
      }
      // Applying a policy can change this caller's authorized rows, but the
      // returned status is already current; avoid an immediate duplicate read.
      const refreshed = await refresh(true, false);
      if (!refreshed) {
        recordDiagnostic({ operationId, feature: "vault", action: "apply_policy", stage: "windows_readback", lifecycle: "verified", outcome: "failed", errorCode: "VLT.POLICY.READBACK_FAILED", severity: "warn", retryability: "manual", suggestedNextAction: "refresh_status", privacyClass: "local_sensitive" });
        showError("Vault settings were saved, but current access could not be verified. Refresh before mounting.", undefined, { operationId });
        return;
      }
      if (removed) {
        recordDiagnostic({ operationId, feature: "vault", action: "apply_policy", stage: "applied", lifecycle: "applied", outcome: "succeeded", severity: "info", retryability: "never", suggestedNextAction: "none", privacyClass: "local_sensitive" });
        showSuccess("Vault policy removed and shared access revoked.", undefined, { operationId });
      } else if (appliedStatus.validation_state === "degraded") {
        recordDiagnostic({ operationId, feature: "vault", action: "apply_policy", stage: "applied", lifecycle: "applied", outcome: "degraded", errorCode: "VLT.POLICY.DEGRADED", severity: "warn", retryability: "manual", suggestedNextAction: "review_status", privacyClass: "local_sensitive" });
        showError("Vault settings were saved with warnings. Fix the listed access problems before mounting.", undefined, { operationId });
      } else {
        recordDiagnostic({ operationId, feature: "vault", action: "apply_policy", stage: "verified", lifecycle: "verified", outcome: "succeeded", severity: "info", retryability: "never", suggestedNextAction: "none", privacyClass: "local_sensitive" });
        showSuccess("Vault settings saved. Future mounts only need the password.", undefined, { operationId });
      }
    } catch (cause) {
      const failure = vaultPolicySaveFailure(cause);
      recordDiagnostic({ operationId, feature: "vault", action: "apply_policy", stage: "applied", lifecycle: "applied", outcome: "failed", errorCode: failure.code, severity: "error", retryability: "manual", suggestedNextAction: failure.code === "VLT.POLICY.ELEVATION_REQUIRED" ? "restart_elevated" : "retry", privacyClass: "local_sensitive" });
      showError(failure.message, undefined, { operationId });
    } finally {
      saveInProgress.current = false;
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
      const current = await getPolicy();
      const loaded = current ? normalizeVaultAccessPolicy(current) : null;
      replacePolicy(loaded, false);
      await refresh(true);
    } catch {
      showError("Saved Vault settings could not be reloaded. Your draft is still available.");
    }
  };

  const rebaseDraft = async () => {
    const draft = policyRef.current;
    if (!draft) return;
    try {
      const current = await getPolicy();
      const latest = current ? normalizeVaultAccessPolicy(current) : null;
      if (!latest) {
        return void showError("The saved Vault policy was removed. Your draft was kept; create a new policy or discard the draft.");
      }
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

  const openMountPrompt = (entry: { entry_id?: string; id?: string; container_kind: VaultContainerKind; access?: VaultAccess }) => {
    const entryId = entry.entry_id ?? entry.id;
    if (!entryId) return;
    setVolumeRole("outer");
    setMountTarget({ entryId, containerKind: entry.container_kind ?? "standard", access: entry.access });
  };

  const mountSelectedEntry = async () => {
    const entryId = mountTarget?.entryId;
    const requestedRole = mountTarget?.containerKind === "dual" ? volumeRole : "outer";
    const input = passwordInputRef.current;
    const hiddenProtectionInput = hiddenProtectionPasswordInputRef.current;
    let password = input?.value ?? "";
    const requiresHiddenProtection = mountTarget?.containerKind === "dual"
      && requestedRole === "outer"
      && mountTarget.access === "write";
    let hiddenProtectionPassword = requiresHiddenProtection ? hiddenProtectionInput?.value ?? "" : "";
    if (!entryId || !password) return void showError("Enter the vault password to mount it.");
    if (requiresHiddenProtection && !hiddenProtectionPassword) return void showError("Enter the hidden password to protect the hidden volume while writing.");

    // Clear the DOM field before awaiting IPC. The local stays only for this request.
    if (input) input.value = "";
    if (hiddenProtectionInput) hiddenProtectionInput.value = "";
    setMountTarget(null);
    setMountingEntryId(entryId);
    const operationId = newDiagnosticOperationId("vault");
    recordDiagnostic({ operationId, feature: "vault", action: "mount", stage: "requested", lifecycle: "requested", outcome: "started", severity: "info", retryability: "never", suggestedNextAction: "none", privacyClass: "local_sensitive" });
    try {
      const mountRequest = mountEntry(entryId, password, requestedRole, hiddenProtectionPassword || undefined, operationId);
      password = "";
      hiddenProtectionPassword = "";
      const result = await mountRequest;
      recordMountResult(result);
      if (result.state === "mounted") {
        recordDiagnostic({ operationId, feature: "vault", action: "mount", stage: "applied", lifecycle: "applied", outcome: "succeeded", severity: "info", retryability: "never", suggestedNextAction: "none", privacyClass: "local_sensitive" });
        showSuccess(vaultMountResultLabel(result), undefined, { operationId });
      } else {
        recordDiagnostic({ operationId, feature: "vault", action: "mount", stage: "applied", lifecycle: "applied", outcome: "failed", errorCode: "VLT.MOUNT.FAILED", severity: "error", retryability: "manual", suggestedNextAction: "review_status", privacyClass: "local_sensitive" });
        showError(vaultMountResultLabel(result), undefined, { operationId });
      }
    } catch {
      recordDiagnostic({ operationId, feature: "vault", action: "mount", stage: "applied", lifecycle: "applied", outcome: "failed", errorCode: "VLT.MOUNT.FAILED", severity: "error", retryability: "manual", suggestedNextAction: "retry", privacyClass: "local_sensitive" });
      showError("The Vault mount request could not be completed.");
    } finally {
      password = "";
      hiddenProtectionPassword = "";
      setMountingEntryId(null);
    }
  };

  const unmountSelectedEntry = async (entryId: string) => {
    setUnmountingEntryId(entryId);
    const operationId = newDiagnosticOperationId("vault");
    recordDiagnostic({ operationId, feature: "vault", action: "dismount", stage: "requested", lifecycle: "requested", outcome: "started", severity: "info", retryability: "never", suggestedNextAction: "none", privacyClass: "local_sensitive" });
    try {
      const result = await unmountEntry(entryId, operationId);
      recordMountResult(result);
      if (result.state === "unmounted") {
        recordDiagnostic({ operationId, feature: "vault", action: "dismount", stage: "applied", lifecycle: "applied", outcome: "succeeded", severity: "info", retryability: "never", suggestedNextAction: "none", privacyClass: "local_sensitive" });
        showSuccess(vaultMountResultLabel(result), undefined, { operationId });
      } else {
        recordDiagnostic({ operationId, feature: "vault", action: "dismount", stage: "applied", lifecycle: "applied", outcome: "failed", errorCode: "VLT.DISMOUNT.FAILED", severity: "error", retryability: "automatic", suggestedNextAction: "retry_cleanup", privacyClass: "local_sensitive" });
        showError(vaultMountResultLabel(result), undefined, { operationId });
      }
    } catch {
      recordDiagnostic({ operationId, feature: "vault", action: "dismount", stage: "applied", lifecycle: "applied", outcome: "failed", errorCode: "VLT.DISMOUNT.FAILED", severity: "error", retryability: "automatic", suggestedNextAction: "retry_cleanup", privacyClass: "local_sensitive" });
      showError("The Vault unmount request could not be completed.", undefined, { operationId });
    } finally {
      setUnmountingEntryId(null);
    }
  };

  if (loading) return <div className="fleet-admin-stack">Loading your Vault access…</div>;
  const activePolicy = policy;
  const authorizedById = new Map(authorizedEntries.map(entry => [entry.entry_id, entry]));
  const policyEntries = activePolicy?.entries ?? [];
  const selectedEntry = policyEntries.find(entry => entry.id === selectedEntryId) ?? policyEntries[0] ?? null;
  const verification = draftDirty ? null : vaultPolicyVerification(status);
  const mountTargetEntry = authorizedEntries.find(entry => entry.entry_id === mountTarget?.entryId)
    ?? activePolicy?.entries.find(entry => entry.id === mountTarget?.entryId);

  return (
    <div className="fleet-admin-stack">
      <Card>
        <CardHeader>
          <CardTitle>{canManagePolicy ? "Saved vaults" : "My vaults"}</CardTitle>
          <CardDescription>
            {canManagePolicy
              ? "Saved vault settings stay on this PC. Mounting asks only for the password, which is never stored."
              : "Only Vaults that the service has authorized for this Windows account appear here; mounting asks only for the password."}
          </CardDescription>
        </CardHeader>
        <CardContent className="fleet-admin-stack">
          {authorizedEntries.length === 0 && (
            <p className="fleet-field-hint">{canManagePolicy ? "No vault has been saved and assigned to this account yet." : "No Vault access is currently assigned to this Windows account."}</p>
          )}
          {authorizedEntries.map(entry => {
            const mountGate = vaultMountGate({ authorized: entry, entryResult: status?.entries.find(item => item.id === entry.entry_id)?.result, draftDirty: false });
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
                        : mountGate.disabledReason ?? "Ready to mount when needed"}
                </p>
              </div>
              {isMounted ? (
                <Button variant="outline" size="sm" disabled={unmountingEntryId === entry.entry_id} onClick={() => void unmountSelectedEntry(entry.entry_id)}>
                  {unmountingEntryId === entry.entry_id ? "Unmounting…" : "Unmount"}
                </Button>
              ) : (
                <Button variant="primary" size="sm" disabled={saving || mountingEntryId === entry.entry_id || !mountGate.canMount} title={mountGate.disabledReason ?? undefined} onClick={() => openMountPrompt(entry)}>
                  {mountingEntryId === entry.entry_id ? "Mounting…" : "Mount"}
                </Button>
              )}
            </div>;
          })}
          <div className="fleet-vault-refresh-row">
            <Button variant="outline" size="sm" disabled={saving} onClick={() => void refresh()}><Icon icon="refresh" size={14} />Refresh</Button>
          </div>
        </CardContent>
      </Card>

      {isAdmin && !canManagePolicy && <div className="fleet-vault-verification-warning" role="alert">
        <Icon icon="warning-sign" size={16} />
        <div><strong>Run WinCommander as administrator to change Vault settings</strong><p>This account is an Administrator, but this app instance is not elevated. You can still view and mount Vaults assigned to this account.</p></div>
      </div>}

      {canManagePolicy && policyLoadUnavailable && <div className="fleet-vault-verification-warning" role="alert">
        <Icon icon="warning-sign" size={16} />
        <div><strong>Vault settings could not be loaded yet</strong><p>Your administrator permission is confirmed, but the local service did not return the saved settings. Refresh this page before making changes; your assigned Vaults can still be mounted normally.</p></div>
      </div>}

      {canManagePolicy && !policyLoadUnavailable && <Card className="fleet-vault-management">
        <CardHeader>
          <div className="fleet-vault-management-header">
            <div>
              <CardTitle>Manage saved Vaults</CardTitle>
              <CardDescription>Open an existing Vault to edit its details or Windows access. Removing a policy never deletes its encrypted container file.</CardDescription>
            </div>
            <Button variant="outline" size="sm" onClick={() => {
              setExistingVaultPath("");
              setExistingVaultLabel("");
              setExistingVaultDialogOpen(true);
            }}><Icon icon="plus" size={14} />Add existing Vault</Button>
          </div>
        </CardHeader>
        <CardContent>
          {policyEntries.length === 0 ? <div className="fleet-vault-empty-inline">
            <Icon icon="database" size={20} />
            <div><strong>No saved Vaults</strong><small>Add an existing encrypted container, or create a new Vault policy below.</small></div>
          </div> : <div className="fleet-vault-policy-grid-wrap">
            <table className="fleet-vault-policy-grid">
              <thead><tr>
                <th scope="col">Vault</th><th scope="col">Container path</th><th scope="col">Scope</th><th scope="col">Allowed users / groups</th><th scope="col">Mounted</th><th scope="col">Health</th><th scope="col">Actions</th>
              </tr></thead>
              <tbody>{policyEntries.map(entry => {
                const authorized = authorizedById.get(entry.id);
                const result = status?.entries.find(item => item.id === entry.id)?.result;
                const mounted = authorized?.mount_state === "mounted";
                return <tr className={selectedEntry?.id === entry.id ? "is-selected" : ""} key={entry.id}>
                  <td><strong>{entry.label}</strong></td>
                  <td className="fleet-vault-policy-path" title={entry.container_path}>{entry.container_path}</td>
                  <td>{entry.mount.presentation === "machine" ? "Shared" : "Personal"}</td>
                  <td title={entry.grants.map(grant => grant.principal_name).join(", ")}>{entry.grants.map(grant => `${grant.principal_name} (${grant.access === "write" ? "edit" : "view"})`).join(", ")}</td>
                  <td>{mounted ? authorized?.drive_letter ?? "Mounted" : "Not mounted"}</td>
                  <td className={result && result !== "applied" ? "is-warning" : ""}>{result ? vaultEntryResultLabel(result) : "Not yet verified"}</td>
                  <td><div className="fleet-vault-policy-actions">
                    <Button variant="outline" size="sm" onClick={() => openEntryEditor(entry.id, "details")}>Edit</Button>
                    <Button variant="outline" size="sm" onClick={() => openEntryEditor(entry.id, "access")}>Manage access</Button>
                    {mounted ? <Button variant="outline" size="sm" disabled={unmountingEntryId === entry.id} onClick={() => void unmountSelectedEntry(entry.id)}>{unmountingEntryId === entry.id ? "Unmounting…" : "Dismount"}</Button>
                      : <Button variant="primary" size="sm" disabled={saving || mountingEntryId === entry.id || !authorized} onClick={() => { if (authorized) openMountPrompt(authorized); }}>{mountingEntryId === entry.id ? "Mounting…" : "Mount"}</Button>}
                    <Button variant="outline" size="sm" onClick={() => removeEntry(entry.id)}>Remove policy</Button>
                  </div></td>
                </tr>;
              })}</tbody>
            </table>
          </div>}
        </CardContent>
      </Card>}

      {canManagePolicy && !policyLoadUnavailable && <fieldset disabled={saving} className="contents">
      <Card>
        <CardHeader>
          <CardTitle>Vault access</CardTitle>
          <CardDescription>Choose who can access this vault. Save vault settings to apply your changes.</CardDescription>
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
          {activePolicy && draftDirty && status && <div className="fleet-vault-verification-warning" role="status">
            <Icon icon="info-sign" size={16} />
            <div>
              <strong>Saved Vault settings are available</strong>
              <p>Windows already has a saved Vault policy. Your local draft has not been applied yet.</p>
              <Button variant="outline" size="sm" onClick={() => void discardDraftAndReload()}>Show saved settings</Button>
            </div>
          </div>}
          {selectedEntry && (() => {
            const entry = selectedEntry;
            const entryIndex = policyEntries.findIndex(candidate => candidate.id === entry.id);
            const authorized = authorizedById.get(entry.id);
            const mountResult = mountResults[entry.id];
            const isMounted = mountResult?.state === "mounted" || authorized?.mount_state === "mounted";
            const entryResult = status?.entries.find(item => item.id === entry.id)?.result;
            const mountGate = vaultMountGate({ authorized, entryResult, draftDirty });
            return <div className="fleet-vault-workspace" key={entry.id} ref={editorRef} data-vault-editor-mode={editorMode}>
              <div className="fleet-vault-workspace-header">
                <div><span className="fleet-vault-step">{editorMode === "access" ? "Manage access" : "Vault details"}</span><strong>{entry.label || `Vault ${entryIndex + 1}`}</strong></div>
                <Button variant="outline" size="sm" onClick={() => removeEntry(entry.id)}>Remove</Button>
              </div>
              <VaultAccessEditor
                key={editorMode}
                entry={entry}
                entryIndex={entryIndex}
                directory={directory}
                onEntryChange={patch => updateEntry(entry.id, patch)}
                onOwnerChange={owner => setOwnerAccount(entry.id, owner)}
                onPresetChange={preset => setAccessPreset(entry.id, preset)}
              />
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
                          : mountGate.disabledReason ?? "Ready to mount when needed"}
                  </p>
                </div>
                {isMounted ? (
                  <Button variant="outline" size="sm" disabled={unmountingEntryId === entry.id} onClick={() => void unmountSelectedEntry(entry.id)}>
                    {unmountingEntryId === entry.id ? "Unmounting…" : "Unmount"}
                  </Button>
                ) : (
                  <Button
                    variant="primary"
                    size="sm"
                    disabled={saving || mountingEntryId === entry.id || !mountGate.canMount}
                    title={mountGate.disabledReason ?? undefined}
                    onClick={() => { if (authorized) openMountPrompt(authorized); }}
                  >
                    {mountingEntryId === entry.id ? "Mounting…" : "Mount"}
                  </Button>
                )}
              </div>
            </div>;
          })()}
          <div className="fleet-action-row">
            <Button variant="outline" onClick={() => addVaultEntryDraft("private")}>Add private vault</Button>
            <Button variant="outline" onClick={() => addVaultEntryDraft("shared")}>Add shared vault</Button>
            {policy && <Button variant="primary" disabled={saving || !!error} onClick={() => policy.entries.length === 0 ? setPolicyRemovalConfirmation(true) : void apply()}>{saving ? "Saving…" : policy.entries.length === 0 ? "Remove Vault policy" : "Save vault settings"}</Button>}
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

      </fieldset>}

      <Dialog open={existingVaultDialogOpen} onOpenChange={open => {
        setExistingVaultDialogOpen(open);
        if (!open) {
          setExistingVaultPath("");
          setExistingVaultLabel("");
        }
      }}>
        <DialogContent className="max-w-lg">
          <DialogHeader>
            <DialogTitle>Add existing Vault</DialogTitle>
            <DialogDescription>
              Select or enter an encrypted container file already on this PC. A filename extension is not required. WinCommander checks the file path and access when you save; it verifies the encrypted container when you unlock it.
            </DialogDescription>
          </DialogHeader>
          <label className="fleet-field"><span>Container file</span>
            <div className="fleet-vault-existing-path">
              <Input aria-label="Existing Vault container path" value={existingVaultPath} placeholder="D:\\Vault\\sales" onChange={event => setExistingVaultPath(event.target.value)} />
              <Button variant="outline" type="button" onClick={() => void browseExistingVault()}>Browse</Button>
            </div>
            <small>Choose the encrypted file itself, not its parent folder. Existing files do not need a particular extension.</small>
          </label>
          <label className="fleet-field"><span>Vault name</span><Input aria-label="Existing Vault label" value={existingVaultLabel} placeholder="Sales" onChange={event => setExistingVaultLabel(event.target.value)} /><small>This is only the label people see in WinCommander.</small></label>
          <DialogFooter>
            <Button variant="outline" onClick={() => setExistingVaultDialogOpen(false)}>Cancel</Button>
            <Button variant="primary" onClick={addExistingVault}>Add and manage access</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={mountTarget !== null} onOpenChange={open => { if (!open) closeMountPrompt(); }}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>Mount {mountTargetEntry?.label ?? "Vault"}</DialogTitle>
            <DialogDescription>
              Enter the password for this mount only. It is cleared before the mount request finishes and is never saved.
            </DialogDescription>
          </DialogHeader>
          {mountTarget?.containerKind === "dual" && <label className="fleet-field"><span>Open</span><select aria-label="Vault volume role" value={volumeRole} onChange={event => setVolumeRole(event.target.value as VaultVolumeRole)}><option value="outer">Outer volume</option><option value="hidden">Hidden volume</option></select><small>Choose the volume for this mount only. The choice and password are never saved.</small></label>}
          {mountTarget?.containerKind === "dual" && volumeRole === "outer" && mountTarget.access === "write" && <Input ref={hiddenProtectionPasswordInputRef} aria-label="Hidden volume protection password" type="password" autoComplete="off" placeholder="Hidden password required to protect it while writing" />}
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

      <Dialog open={policyRemovalConfirmation} onOpenChange={setPolicyRemovalConfirmation}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>Remove the saved Vault policy?</DialogTitle>
            <DialogDescription>
              WinCommander will dismount managed Vaults, revoke their shared Windows access, and remove the saved policy. The encrypted container files are not deleted.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setPolicyRemovalConfirmation(false)}>Cancel</Button>
            <Button variant="primary" onClick={() => { setPolicyRemovalConfirmation(false); void apply(); }}>Remove policy</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
