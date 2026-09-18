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
  const detail = (cause instanceof Error ? cause.message : String(cause ?? "")).toLowerCase();
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

export function vaultPolicySaveFailure(cause: unknown): { code: "VLT.POLICY.ADMIN_ACCESS_REQUIRED" | "VLT.POLICY.VERSION_CONFLICT" | "VLT.POLICY.CONTAINER_UNAVAILABLE" | "VLT.POLICY.PRINCIPAL_UNAVAILABLE" | "VLT.POLICY.ACL_UNVERIFIED" | "VLT.POLICY.ACTIVE_MOUNT" | "VLT.POLICY.SERVICE_UNAVAILABLE" | "VLT.POLICY.INVALID" | "VLT.POLICY.APPLY_FAILED"; message: string } {
  // Keep the service's transport/Windows detail out of the UI.  The service
  // already makes the authorization decision; this only turns its fixed error
  // categories into an action the person can take.
  // Tauri rejects an invoke with a string, not necessarily an Error object.
  // Treat both forms identically so an administrator sees the service's safe
  // category instead of every failure becoming the opaque generic fallback.
  const detail = (cause instanceof Error ? cause.message : String(cause ?? "")).toLowerCase();
  if (detail.includes("forbidden") || detail.includes("privileged") || detail.includes("vault policy administrator")) {
    return {
      code: "VLT.POLICY.ADMIN_ACCESS_REQUIRED",
      message: "This Windows account is not allowed to change Vault settings. Ask a device administrator to add it to WinCommander Vault Policy Administrators. Your assigned Vaults can still be mounted normally.",
    };
  }
  if (detail.includes("version conflict") || detail.includes("changed elsewhere")) {
    return {
      code: "VLT.POLICY.VERSION_CONFLICT",
      message: "Vault settings changed in another WinCommander window. Refresh this page before saving again.",
    };
  }
  if (detail.includes("container identity")) {
    return {
      code: "VLT.POLICY.CONTAINER_UNAVAILABLE",
      message: "The saved Vault container file is missing, moved, or not readable. Choose the encrypted file itself in Edit, then save again.",
    };
  }
  if (detail.includes("principal resolution") || detail.includes("could not find one or more named")) {
    return {
      code: "VLT.POLICY.PRINCIPAL_UNAVAILABLE",
      message: "Windows could not find one of the named users or groups in this Vault permission. In Access control, refresh the Windows users and groups, then choose the account or group again before saving.",
    };
  }
  if (detail.includes("acl") || detail.includes("access plan") || detail.includes("read-back")) {
    return {
      code: "VLT.POLICY.ACL_UNVERIFIED",
      message: "Windows could not set or verify the Vault's file permissions. Keep the container in its own folder, check that the folder still exists, then save again. The previous Vault permission was left unchanged.",
    };
  }
  if (detail.includes("dismount") || detail.includes("active vault")) {
    return {
      code: "VLT.POLICY.ACTIVE_MOUNT",
      message: "WinCommander could not safely dismount an active Vault before changing its permission. Close files using that Vault, dismount it, then save again. The existing permission was left unchanged.",
    };
  }
  if (detail.includes("timed out") || detail.includes("pipe") || detail.includes("connect") || detail.includes("service unavailable") || detail.includes("reply")) {
    return {
      code: "VLT.POLICY.SERVICE_UNAVAILABLE",
      message: "The local WinCommander service is still starting or unavailable, so no Vault settings were changed. Wait a moment, refresh the Vault page, then save again.",
    };
  }
  if (detail.includes("validation") || detail.includes("invalid") || detail.includes("duplicate") || detail.includes("persist")) {
    return {
      code: "VLT.POLICY.INVALID",
      message: "Windows rejected these Vault settings before saving them. Check the container file, its dedicated parent folder, the selected users or groups, and any preferred drive letter, then save again.",
    };
  }
  return {
    code: "VLT.POLICY.APPLY_FAILED",
    message: "The local service did not save these Vault settings. No existing permission was changed. Refresh the Vault page and retry; use the reference below if it repeats.",
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
  const [entryRemovalConfirmation, setEntryRemovalConfirmation] = useState<string | null>(null);
  const [policyRemovalConfirmation, setPolicyRemovalConfirmation] = useState(false);
  const [forgetPolicyConfirmation, setForgetPolicyConfirmation] = useState<string | null>(null);
  const [existingVaultDialogOpen, setExistingVaultDialogOpen] = useState(false);
  const [existingVaultPath, setExistingVaultPath] = useState("");
  const [existingVaultLabel, setExistingVaultLabel] = useState("");
  const [selectedEntryId, setSelectedEntryId] = useState<string | null>(initialDraft?.policy?.entries[0]?.id ?? null);
  const [editorMode, setEditorMode] = useState<"details" | "access">("details");
  // A saved policy is primarily a status table.  Keep the editable access
  // workspace closed until the administrator deliberately chooses Edit or
  // Manage access for one Vault, so opening this tab cannot invite an
  // accidental policy change.
  const [editorOpen, setEditorOpen] = useState(false);
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
  const { getPolicy, getStatus, applyPolicy, forgetPolicy, mountEntry, unmountEntry, listAuthorizedEntries, getCapabilities } = useVaultAccess<VaultAccessPolicy, VaultPolicyStatus>();
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
    setEditorOpen(true);
    window.requestAnimationFrame(() => {
      const details = editorRef.current?.querySelector<HTMLDetailsElement>(".vault-access-details");
      if (details) details.open = false;
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

  const removeEntryDraft = (id: string) => {
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

  const requestEntryRemoval = (id: string) => {
    // A row created only in this unsaved draft has no service policy to
    // revoke. Removing it locally is safe; a saved row must go through the
    // confirmation and service-backed apply below.
    if (!draftBaseRef.current?.entries.some(entry => entry.id === id)) {
      removeEntryDraft(id);
      return;
    }
    setEntryRemovalConfirmation(id);
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

  const apply = async (
    policyToApply = policy,
    policyEntryRemoved = false,
    draftToKeepAfterSave: VaultAccessPolicy | null = null,
  ) => {
    if (saveInProgress.current) return;
    if (!policyToApply) return;
    const policyError = validateVaultAccessIntent(policyToApply);
    if (policyError) return void showError(policyError);
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
      const submittedPolicy = nextVaultAccessPolicy(policyToApply);
      // Keep the UI reference, desktop diagnostic and service event correlated.
      // Without this, a support reference could not identify the failed save.
      const appliedStatus = await applyPolicy(submittedPolicy, operationId);
      const removed = submittedPolicy.entries.length === 0;
      const keepDraft = draftToKeepAfterSave !== null;
      // A successful full removal leaves no saved policy to edit. Clear every
      // policy-derived selection and editor state before the readback, so a
      // delayed refresh can never leave a deleted row visible or editable.
      // Deliberately retain an unrelated local draft: it has not been sent to
      // the service and is explained to the administrator as a draft below.
      const savedPolicyRemoved = removed && !keepDraft;
      // Removing one saved Vault is intentionally a surgical operation.  A
      // separate edit the administrator has not saved yet remains a local
      // draft rather than being silently sent with the removal request.
      replacePolicy(keepDraft ? draftToKeepAfterSave : removed ? null : submittedPolicy, keepDraft, submittedPolicy);
      if (savedPolicyRemoved) {
        setStatus(null);
        setAuthorizedEntries([]);
        setMountResults({});
        setSelectedEntryId(null);
        setEditorMode("details");
        setEditorOpen(false);
        setEntryRemovalConfirmation(null);
        setPolicyRemovalConfirmation(false);
      } else {
        setStatus(appliedStatus);
      }
      // Applying a policy can change this caller's authorized rows, but the
      // returned status is already current; avoid an immediate duplicate read.
      const refreshed = await refresh(!keepDraft, false);
      if (!refreshed) {
        recordDiagnostic({ operationId, feature: "vault", action: "apply_policy", stage: "windows_readback", lifecycle: "verified", outcome: "failed", errorCode: "VLT.POLICY.READBACK_FAILED", severity: "warn", retryability: "manual", suggestedNextAction: "refresh_status", privacyClass: "local_sensitive" });
        showError("Vault settings were saved, but current access could not be verified. Refresh before mounting.", undefined, { operationId });
        return;
      }
      if (removed) {
        recordDiagnostic({ operationId, feature: "vault", action: "apply_policy", stage: "applied", lifecycle: "applied", outcome: "succeeded", severity: "info", retryability: "never", suggestedNextAction: "none", privacyClass: "local_sensitive" });
        showSuccess("Vault policy removed and shared access revoked.", undefined, { operationId });
      } else if (policyEntryRemoved) {
        recordDiagnostic({ operationId, feature: "vault", action: "apply_policy", stage: "applied", lifecycle: "applied", outcome: "succeeded", severity: "info", retryability: "never", suggestedNextAction: "none", privacyClass: "local_sensitive" });
        showSuccess("Vault removed from saved policy. It can now use normal Secure Storage mounting with its password.", undefined, { operationId });
      } else if (appliedStatus.validation_state === "degraded") {
        recordDiagnostic({ operationId, feature: "vault", action: "apply_policy", stage: "applied", lifecycle: "applied", outcome: "degraded", errorCode: "VLT.POLICY.DEGRADED", severity: "warn", retryability: "manual", suggestedNextAction: "review_status", privacyClass: "local_sensitive" });
        showError("Vault settings were saved with warnings. Fix the listed access problems before mounting.", undefined, { operationId });
      } else {
        recordDiagnostic({ operationId, feature: "vault", action: "apply_policy", stage: "verified", lifecycle: "verified", outcome: "succeeded", severity: "info", retryability: "never", suggestedNextAction: "none", privacyClass: "local_sensitive" });
        showSuccess("Vault settings saved. Future mounts only need the password.", undefined, { operationId });
      }
    } catch (cause) {
      const failure = vaultPolicySaveFailure(cause);
      recordDiagnostic({ operationId, feature: "vault", action: "apply_policy", stage: "applied", lifecycle: "applied", outcome: "failed", errorCode: failure.code, severity: "error", retryability: "manual", suggestedNextAction: failure.code === "VLT.POLICY.ADMIN_ACCESS_REQUIRED" ? "request_admin_access" : "retry", privacyClass: "local_sensitive" });
      showError(failure.message, undefined, { operationId });
    } finally {
      saveInProgress.current = false;
      setSaving(false);
    }
  };

  const confirmEntryRemoval = () => {
    const entryId = entryRemovalConfirmation;
    setEntryRemovalConfirmation(null);
    const current = policyRef.current;
    const saved = draftBaseRef.current;
    if (!entryId || !current || !saved) return;
    // Apply the deletion against the last service-owned policy, not the
    // mutable editor state. This makes the selected row the only server-side
    // change made by this confirmation.
    const next = removeVaultEntryDraft(saved, entryId, true);
    if (!next) return;
    const remainingDraft = removeVaultEntryDraft(current, entryId, true);
    const draftToKeepAfterSave = dirtyRef.current && remainingDraft
      ? {
        ...remainingDraft,
        // Clearing the last saved entry leaves no active policy. A later
        // draft save must start again at version zero; otherwise its next
        // write would be rejected as a stale policy version.
        version: next.entries.length === 0 ? 0 : next.version,
        expected_previous_version: next.entries.length === 0 ? 0 : next.version,
      }
      : null;
    void apply(next, true, draftToKeepAfterSave);
  };

  const repairSharedAccess = () => {
    const saved = draftBaseRef.current;
    if (!saved) return void showError("Saved Vault settings are not available yet. Refresh this page before repairing shared access.");
    // Reapply the last service-owned policy, never a local draft. A draft
    // stays local and is restored after the service has retried Windows ACLs.
    const draft = policyRef.current;
    const draftToKeep = dirtyRef.current && draft
      ? {
        ...draft,
        // The repair write advances the saved revision even if Windows still
        // reports it degraded. Keep a later draft save based on that revision
        // rather than turning the recovery into a guaranteed conflict.
        version: saved.version + 1,
        expected_previous_version: saved.version + 1,
      }
      : null;
    void apply(saved, false, draftToKeep);
  };

  const forgetSavedPolicy = async () => {
    if (saveInProgress.current) return;
    const entryId = forgetPolicyConfirmation;
    if (!entryId) return;
    const saved = draftBaseRef.current;
    if (!saved || !saved.entries.some(entry => entry.id === entryId)) {
      return void showError("Saved Vault settings changed before this recovery action. Refresh the page and try again.");
    }
    saveInProgress.current = true;
    setSaving(true);
    const operationId = newDiagnosticOperationId("vault");
    recordDiagnostic({ operationId, feature: "vault", action: "forget_policy", stage: "requested", lifecycle: "requested", outcome: "started", severity: "warn", retryability: "never", suggestedNextAction: "none", privacyClass: "local_sensitive" });
    try {
      // The service owns the safety boundary: this recovery command removes
      // only this record and explicitly does not change its Windows ACLs.
      await forgetPolicy(entryId, saved.policy_id, saved.version, operationId);
      // A local draft still references the old service revision. Discard it
      // before reading the updated policy so it cannot later recreate the
      // deliberately forgotten entry.
      replacePolicy(null, false);
      setStatus(null);
      setAuthorizedEntries([]);
      setMountResults({});
      setSelectedEntryId(null);
      setEditorOpen(false);
      setForgetPolicyConfirmation(null);
      await refresh(true);
      recordDiagnostic({ operationId, feature: "vault", action: "forget_policy", stage: "applied", lifecycle: "applied", outcome: "succeeded", severity: "warn", retryability: "never", suggestedNextAction: "none", privacyClass: "local_sensitive" });
      showSuccess("Vault removed from WinCommander policy. Windows file permissions were left unchanged.", undefined, { operationId });
    } catch (cause) {
      const failure = vaultPolicySaveFailure(cause);
      recordDiagnostic({ operationId, feature: "vault", action: "forget_policy", stage: "applied", lifecycle: "applied", outcome: "failed", errorCode: failure.code, severity: "error", retryability: "manual", suggestedNextAction: "retry", privacyClass: "local_sensitive" });
      showError("WinCommander could not forget this Vault policy entry. Windows file permissions were not changed.", undefined, { operationId });
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
    setEditorOpen(true);
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
    if (action === "replace") {
      replaceWithSharedDraft();
      setEditorOpen(true);
    }
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
      showError("The Vault mount request could not be completed.", undefined, { operationId });
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
            <div className="fleet-vault-empty-inline" data-vault-empty-state="expanded">
              <Icon icon="database" size={20} />
              <div>
                <strong>{canManagePolicy ? "No saved Vaults" : "No Vaults are assigned to this account"}</strong>
                <small>{canManagePolicy
                  ? "No Vault has been saved and assigned to this account yet."
                  : "An administrator must grant this Windows account access before a Vault can appear here. Refresh after they make the change."}</small>
              </div>
            </div>
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
        <div><strong>This account needs Vault policy-manager access</strong><p>Ask a device administrator to add this Windows account to WinCommander Vault Policy Administrators. You can still view and mount Vaults assigned to this account.</p></div>
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
                const canRepairSharedAccess = result === "acl_apply_failed" || result === "acl_readback_failed";
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
                    {canRepairSharedAccess && <Button variant="outline" size="sm" disabled={saving} onClick={repairSharedAccess}>Repair shared access</Button>}
                    {canRepairSharedAccess && <Button variant="outline" size="sm" disabled={saving} onClick={() => setForgetPolicyConfirmation(entry.id)}>Forget policy…</Button>}
                    <Button variant="outline" size="sm" onClick={() => requestEntryRemoval(entry.id)}>Remove policy</Button>
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
          <div className="fleet-vault-management-header">
            <div>
              <CardTitle>Vault access</CardTitle>
              <CardDescription>{editorOpen ? "Choose who can access this vault. Save vault settings to apply your changes." : "Choose Edit or Manage access on a saved Vault to open its policy."}</CardDescription>
            </div>
            {editorOpen && <Button variant="outline" size="sm" onClick={() => setEditorOpen(false)}>Close editor</Button>}
          </div>
        </CardHeader>
        {!editorOpen ? <CardContent className="fleet-admin-stack">
          {!activePolicy ? <div className="fleet-vault-empty-setup">
            <div><strong>No vault access is configured yet</strong><span>Create a Vault policy to start assigning Windows users or groups.</span></div>
            <div className="fleet-action-row">
              <Button onClick={createSharedDraft}>Create first shared vault</Button>
              <Button variant="outline" onClick={() => { editPolicy(current => current ?? newVaultPolicy()); setEditorOpen(true); }}>Use three-vault starter</Button>
            </div>
          </div> : <p className="fleet-field-hint">Policies stay collapsed until you choose Edit or Manage access for a saved Vault above.</p>}
        </CardContent> : <CardContent className="fleet-admin-stack">
          {!activePolicy && <div className="fleet-vault-empty-setup">
            <div><strong>No vault access is configured yet</strong><span>Start with one shared vault, or use the recommended personal-and-shared starter.</span></div>
            <div className="fleet-action-row">
              <Button onClick={createSharedDraft}>Create first shared vault</Button>
              <Button variant="outline" onClick={() => { editPolicy(current => current ?? newVaultPolicy()); setEditorOpen(true); }}>Use three-vault starter</Button>
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
                <Button variant="outline" size="sm" onClick={() => requestEntryRemoval(entry.id)}>Remove</Button>
              </div>
              <VaultAccessEditor
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
            <p>Use these only to import an older planner draft, discard local edits, repair a degraded shared-access policy, or remove an unrecoverable policy record without changing Windows permissions.</p>
            <div className="fleet-action-row">
              <Button variant="outline" size="sm" onClick={importLegacyDraft}>Import retired planner as draft</Button>
              {draftDirty && <Button variant="outline" size="sm" onClick={() => void rebaseDraft()}>Rebase draft with saved settings</Button>}
              <Button variant="outline" size="sm" onClick={() => void discardDraftAndReload()}>Discard draft & reload saved</Button>
              {status?.validation_state === "degraded" && <Button variant="outline" size="sm" onClick={repairSharedAccess}>Repair shared access</Button>}
            </div>
            {legacyNotice && <span className="fleet-field-hint">{legacyNotice}</span>}
          </details>
        </CardContent>}
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

      <Dialog open={entryRemovalConfirmation !== null} onOpenChange={open => { if (!open) setEntryRemovalConfirmation(null); }}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>Remove this Vault from the saved policy?</DialogTitle>
            <DialogDescription>
              WinCommander will dismount only this Vault if needed, revoke only its shared Windows access, and save that removal. Other unsaved edits stay as a local draft and are not included. The encrypted container file is not deleted; it can then be mounted through Secure Storage with normal Windows access and its password.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setEntryRemovalConfirmation(null)}>Cancel</Button>
            <Button variant="primary" onClick={confirmEntryRemoval}>Remove and save</Button>
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

      <Dialog open={forgetPolicyConfirmation !== null} onOpenChange={open => { if (!open) setForgetPolicyConfirmation(null); }}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>Forget this degraded Vault policy?</DialogTitle>
            <DialogDescription>
              This removes only this Vault&apos;s WinCommander policy record. It does not revoke or repair any Windows file permissions, does not delete the encrypted container, and may leave the current users or groups with their existing Windows access. Other saved Vault policies are not changed. Any unsaved local Vault draft will be discarded. Use Repair shared access first whenever possible.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setForgetPolicyConfirmation(null)}>Cancel</Button>
            <Button variant="primary" onClick={() => void forgetSavedPolicy()}>Forget policy; leave Windows permissions unchanged</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
