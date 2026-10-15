// SPDX-License-Identifier: AGPL-3.0-or-later
import { useCallback, useEffect, useId, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-shell";
import { Button, Dialog, DialogBody, DialogFooter, Icon } from "@/components/ui/bp";
import { Button as UiButton } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import "./PersonalVaultSyncDialog.css";
import useBackend from "@/hooks/useBackend";
import type { VaultSyncFolder } from "@/types/generated/ipc";
import { isSyncthingSetupUrl } from "@/lib/vaultSyncRecovery";
import { notifyVaultSyncRecovery } from "@/lib/vaultSyncWarning";
import { personalVaultSyncError, personalVaultSyncOutcomeUnknown, personalVaultSyncSetupMessage } from "@/lib/personalVaultSyncFeedback";
import { DEFAULT_VAULT_SYNC_LABEL, validVaultSyncLabel, validateVaultSyncDrafts, vaultSyncLabel, acceptsVaultSyncMountReceipt, type VaultSyncDraft } from "@/lib/personalVaultSyncManagement";
import { showSuccess } from "@/utils/toast";
import VaultOperationNotice from "./VaultOperationNotice";

interface Props { isOpen: boolean; onClose: () => void; internalDrive: number; driveLabel: string }
interface SyncDraftRow extends VaultSyncDraft { id: number }
export default function PersonalVaultSyncDialog({ isOpen, onClose, internalDrive, driveLabel }: Props) {
  const fieldId = useId();
  const backend = useBackend();
  const latest = useRef(backend); latest.current = backend;
  const generation = useRef(0);
  const inFlight = useRef(false);
  const operationKind = useRef<"change" | "refresh" | "open">("refresh");
  const uncertain = useRef(false);
  const mountReceipt = useRef("");
  const returnFocus = useRef<HTMLElement | null>(null);
  const [folders, setFolders] = useState<VaultSyncFolder[]>([]);
  const nextDraftId = useRef(1);
  const [drafts, setDrafts] = useState<SyncDraftRow[]>([{ id: 0, path: "Sync", label: DEFAULT_VAULT_SYNC_LABEL }]);
  const [labels, setLabels] = useState<Record<string, string>>({});
  const [removeId, setRemoveId] = useState("");
  const [busy, setBusy] = useState(false);
  const [changing, setChanging] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState("");
  const [gui, setGui] = useState("");
  const [showInfo, setShowInfo] = useState(false);
  const [syncDisabled, setSyncDisabled] = useState(false);
  const [canEnableSync, setCanEnableSync] = useState(false);
  const refresh = useCallback(async (epoch = generation.current) => {
    const result = await latest.current.managePersonalVaultSync(internalDrive, "list");
    if (epoch !== generation.current) return;
    if (!acceptsVaultSyncMountReceipt(mountReceipt.current, result.mount_receipt)) {
      setLoaded(false); setDrafts([]); setFolders([]); setGui("");
      throw new Error("vault_sync_mount_changed");
    }
    const initialLoad = mountReceipt.current === "";
    mountReceipt.current = result.mount_receipt;
    setFolders(result.folders);
    if (initialLoad && result.folders.length > 0) {
      setDrafts(result.folders.length >= 32 ? [] : [{ id: 0, path: "", label: DEFAULT_VAULT_SYNC_LABEL }]);
    }
    setSyncDisabled(!result.syncthing_enabled);
    setCanEnableSync(result.can_enable_syncthing);
    setLabels(Object.fromEntries(result.folders.map(folder => [folder.folder_id, folder.label])));
    setGui(isSyncthingSetupUrl(result.gui_url) ? result.gui_url : "");
    setLoaded(true);
  }, [internalDrive]);
  const invalidate = useCallback(() => { generation.current++; }, []);
  useEffect(() => {
    const epoch = ++generation.current;
    if (isOpen) {
      mountReceipt.current = "";
      setLoaded(false); setError(""); setFolders([]); setGui(""); setRemoveId(""); setSyncDisabled(false); setCanEnableSync(false); setBusy(true);
      inFlight.current = true;
      operationKind.current = "refresh"; uncertain.current = false; setChanging(false);
      nextDraftId.current = 1;
      setDrafts([{ id: 0, path: "Sync", label: DEFAULT_VAULT_SYNC_LABEL }]);
      setShowInfo(false);
      void refresh(epoch).catch(failure => {
        if (epoch !== generation.current) return;
        const detail = failure instanceof Error ? failure.message.toLowerCase() : String(failure).toLowerCase();
        setSyncDisabled(detail.includes("vault_syncthing_not_enabled"));
        setCanEnableSync(false);
        setError(personalVaultSyncError(failure, "status"));
      })
        .finally(() => { if (epoch === generation.current) { inFlight.current = false; setBusy(false); } });
    }
    return invalidate;
  }, [isOpen, internalDrive, refresh, invalidate]);
  const perform = async (operation: () => Promise<void>, kind: "change" | "refresh" | "open" = "change") => {
    if (inFlight.current) return;
    if (kind === "change" && (!loaded || uncertain.current)) { setError("Select Refresh to confirm the current sync folders before making another change."); return; }
    const epoch = generation.current;
    inFlight.current = true;
    operationKind.current = kind;
    setBusy(true); setChanging(kind === "change"); setError("");
    try { await operation(); if (kind === "refresh" && epoch === generation.current) uncertain.current = false; }
    catch (failure) { if (epoch === generation.current) {
      if (kind === "refresh") setLoaded(false);
      if (personalVaultSyncOutcomeUnknown(failure)) { uncertain.current = true; setLoaded(false); }
      setError(personalVaultSyncError(failure, kind === "refresh" ? "status" : "setup"));
    } }
    finally { if (epoch === generation.current) { inFlight.current = false; setBusy(false); setChanging(false); } }
  };
  const enable = () => {
    const expectedMountReceipt = mountReceipt.current;
    if (!loaded || !acceptsVaultSyncMountReceipt("", expectedMountReceipt)) {
      setError(personalVaultSyncError("vault_sync_mount_changed")); return;
    }
    const validated = validateVaultSyncDrafts(drafts, folders.map(folder => folder.relative_path));
    if (!validated.ok) { setError(validated.message); return; }
    void perform(async () => {
      try {
      for (const draft of drafts) {
        const path = draft.path.trim().replaceAll("/", "\\");
        const result = await latest.current.enablePersonalVaultSync(internalDrive, path, undefined, undefined, expectedMountReceipt);
        if (result.recovery_required) {
          onClose();
          window.setTimeout(() => notifyVaultSyncRecovery(driveLabel, internalDrive, result), 350);
          return;
        }
        if (!result.enabled || !result.folder_id) throw new Error("vault_broker_rejected");
        // Enrollment is already confirmed even if the separate name update
        // fails. Expose that configured row instead of offering a duplicate add.
        setDrafts(current => current.filter(entry => entry !== draft));
        try {
          await latest.current.managePersonalVaultSync(internalDrive, "rename", path, vaultSyncLabel(draft.label), result.folder_id, expectedMountReceipt);
        } catch (failure) { if (personalVaultSyncOutcomeUnknown(failure)) throw failure; throw new Error("vault_sync_label_update_failed"); }
        showSuccess(personalVaultSyncSetupMessage(`${driveLabel}\\${path}`, result.pairing_required));
      }
      await refresh();
      } catch (failure) {
        // An earlier folder or enrollment may already have succeeded. Re-read
        // those receipts instead of continuing to display "not enabled".
        await refresh().catch(() => setLoaded(false));
        throw failure;
      }
    });
  };
  const enableSyncForVault = () => void perform(async () => {
    await latest.current.managePersonalVaultSync(internalDrive, "enable", undefined, undefined, undefined, mountReceipt.current);
    setSyncDisabled(false);
    await refresh();
  });
  const recover = (folder: VaultSyncFolder) => void perform(async () => {
    const result = await latest.current.enablePersonalVaultSync(internalDrive, folder.relative_path, "inspect", undefined, mountReceipt.current);
    if (result.recovery_required) {
      onClose();
      window.setTimeout(() => notifyVaultSyncRecovery(driveLabel, internalDrive, result), 350);
    } else { await refresh(); }
  });
  const pathsValid = validateVaultSyncDrafts(drafts.map(draft => ({ ...draft, label: DEFAULT_VAULT_SYNC_LABEL })), folders.map(folder => folder.relative_path)).ok;
  const close = () => { if (!inFlight.current || operationKind.current !== "change") onClose(); };
  const addFolder = () => {
    if (inFlight.current || !loaded) return;
    const id = nextDraftId.current++;
    setDrafts(current => folders.length + current.length >= 32 ? current
      : [...current, { id, path: "", label: DEFAULT_VAULT_SYNC_LABEL }]);
  };
  const removeDraft = (id: number) => {
    if (inFlight.current) return;
    const index = drafts.findIndex(draft => draft.id === id);
    const remaining = drafts.filter(draft => draft.id !== id);
    const focusId = remaining[index]?.id ?? remaining[index - 1]?.id;
    setDrafts(remaining);
    window.requestAnimationFrame(() => document.getElementById(focusId === undefined
      ? `${fieldId}-add-folder` : `${fieldId}-path-${focusId}`)?.focus());
  };
  return <Dialog isOpen={isOpen} onClose={close} title="Sync setup" className="personal-vault-sync"
    onOpenAutoFocus={() => { returnFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null; }}
    onCloseAutoFocus={event => { if (returnFocus.current?.isConnected) { event.preventDefault(); returnFocus.current.focus({ preventScroll: true }); } }}
    style={{ width: "min(48rem, calc(100vw - 2rem))" }} canEscapeKeyClose={!changing} canOutsideClickClose={!changing} isCloseButtonShown={!changing}>
    <DialogBody>
      <div className="personal-vault-sync__overview">
        <div className="personal-vault-sync__context"><span>Selected Vault</span><strong>{driveLabel}</strong></div>
        <Tooltip open={showInfo} onOpenChange={setShowInfo}>
          <TooltipTrigger asChild>
            <UiButton variant="ghost" size="icon" className="personal-vault-sync__info" aria-label="About Sync setup"
              onClick={event => { event.preventDefault(); setShowInfo(current => !current); }}><Icon icon="info-sign" size={16} /></UiButton>
          </TooltipTrigger>
          <TooltipContent side="bottom" align="end" className="personal-vault-sync__tooltip">
            After setup, open Sync to share each configured folder with your device. Other sync folders keep running when this Vault is dismounted.
          </TooltipContent>
        </Tooltip>
      </div>
      <p className="personal-vault-sync__intro">Sync selected folders with your phone or another device. Keep this Vault mounted during setup.</p>
      {syncDisabled && <p role="status" className="personal-vault-sync__help">{canEnableSync ? "Sync is off for this Vault. Enable it before adding folders." : "Sync is off for this Vault. Open Vault access, edit this Vault, turn on Sync, and save."}</p>}
      <section aria-labelledby={`${fieldId}-existing`} className="personal-vault-sync__section">
        <h3 id={`${fieldId}-existing`}>Existing synced folders</h3>
        {loaded && folders.length === 0 && <p className="personal-vault-sync__muted" role="status">No folders added yet. Add your first folder below.</p>}
        {folders.map((folder, index) => <div key={folder.folder_id} className="personal-vault-sync__folder">
          <div className="personal-vault-sync__folder-heading">
            <strong>{driveLabel}\{folder.relative_path}</strong>
            <span className="personal-vault-sync__status">{!loaded ? "Status unverified" : folder.recovery_required ? "Needs recovery" : folder.paused ? "Paused" : "Enabled"}</span>
          </div>
          <label htmlFor={`${fieldId}-saved-${index}`}>Sync display name</label>
          <Input id={`${fieldId}-saved-${index}`} value={labels[folder.folder_id] ?? folder.label} maxLength={128} disabled={busy}
            aria-describedby={`${fieldId}-name-help ${fieldId}-saved-error-${index}`} aria-invalid={!validVaultSyncLabel(vaultSyncLabel(labels[folder.folder_id]))}
            onChange={event => setLabels(current => ({ ...current, [folder.folder_id]: event.target.value }))} />
          {!validVaultSyncLabel(vaultSyncLabel(labels[folder.folder_id])) && <p id={`${fieldId}-saved-error-${index}`} role="alert">Use a display name of up to 128 bytes, without control characters. Shorten the name before saving.</p>}
          <div className="personal-vault-sync__actions">
            <Button disabled={busy || !loaded || !validVaultSyncLabel(vaultSyncLabel(labels[folder.folder_id]))} onClick={() => void perform(async () => {
              await latest.current.managePersonalVaultSync(internalDrive, "rename", folder.relative_path, vaultSyncLabel(labels[folder.folder_id]), folder.folder_id, mountReceipt.current);
              await refresh(); showSuccess("Sync folder name updated. Its files and connected devices were kept.");
            })}>Save name</Button>
            {folder.recovery_required && <Button disabled={busy || !loaded} onClick={() => recover(folder)}>Review recovery</Button>}
            <Button disabled={busy || !loaded} onClick={() => setRemoveId(folder.folder_id)}>Remove sync</Button>
          </div>
          {removeId === folder.folder_id && <div role="alert" className="personal-vault-sync__confirmation">
            <p>Remove only this folder’s sync configuration? Files in the Vault and on your device stay in place. Other folders keep syncing.</p>
            <div className="personal-vault-sync__actions">
              <Button disabled={busy || !loaded} intent="danger" onClick={() => void perform(async () => {
                await latest.current.managePersonalVaultSync(internalDrive, "remove", folder.relative_path, undefined, folder.folder_id, mountReceipt.current);
                setRemoveId(""); await refresh(); showSuccess("Sync removed for this folder. Its files were kept.");
              })}>Confirm remove sync</Button>
              <Button disabled={busy} onClick={() => setRemoveId("")}>Keep syncing</Button>
            </div>
          </div>}
        </div>)}
      </section>
      <section aria-labelledby={`${fieldId}-add`} className="personal-vault-sync__section">
        <h3 id={`${fieldId}-add`}>Add a folder</h3>
        <p id={`${fieldId}-path-help`} className="personal-vault-sync__muted">Use a path inside this Vault, for example <code>Phone\Camera</code>. Choose separate folders that do not overlap.</p>
        <p id={`${fieldId}-name-help`} className="personal-vault-sync__muted">The sync name is a label. It does not rename, move or delete files.</p>
        {drafts.map((draft, index) => <div key={draft.id} className="personal-vault-sync__draft">
          <div className="personal-vault-sync__draft-heading"><h4>Folder {index + 1}</h4>
            <Button minimal disabled={busy} aria-label={`Remove folder ${index + 1} from setup`} onClick={() => removeDraft(draft.id)}>Remove</Button>
          </div>
          <div className="personal-vault-sync__fields">
          <div>
            <label htmlFor={`${fieldId}-path-${draft.id}`}>Vault-relative folder path</label>
            <Input id={`${fieldId}-path-${draft.id}`} value={draft.path} maxLength={240} disabled={busy} placeholder="Phone\Camera"
              aria-describedby={`${fieldId}-path-help ${fieldId}-error`} aria-invalid={Boolean(error) && !pathsValid}
              onChange={event => setDrafts(current => current.map((item, i) => i === index ? { ...item, path: event.target.value } : item))} />
          </div>
          <div>
            <label htmlFor={`${fieldId}-label-${draft.id}`}>Sync display name</label>
            <Input id={`${fieldId}-label-${draft.id}`} value={draft.label} maxLength={128} disabled={busy}
              aria-describedby={`${fieldId}-name-help ${fieldId}-error`} aria-invalid={Boolean(error) && !validVaultSyncLabel(vaultSyncLabel(draft.label))}
              onChange={event => setDrafts(current => current.map((item, i) => i === index ? { ...item, label: event.target.value } : item))} />
          </div>
          </div>
        </div>)}
        <Button id={`${fieldId}-add-folder`} disabled={busy || !loaded || folders.length + drafts.length >= 32} onClick={addFolder}>Add another folder</Button>
        {folders.length + drafts.length >= 32 && <p className="personal-vault-sync__muted" role="status">A Vault can have up to 32 separate sync folders.</p>}
      </section>
      {busy && <p role="status">{changing ? "Saving sync changes… Keep this Vault mounted." : "Checking sync folders… You can close this window while the check finishes."}</p>}
      <div id={`${fieldId}-error`} className="personal-vault-sync__feedback"><VaultOperationNotice message={error} /></div>
    </DialogBody>
    <DialogFooter className="personal-vault-sync__footer" actions={<>
      <Button disabled={busy} onClick={() => void perform(() => refresh(), "refresh")}>Refresh</Button>
      {gui && <Button disabled={busy} onClick={() => void perform(() => open(gui), "open")}>Open Sync</Button>}
      <Button disabled={changing} onClick={close}>Close</Button>
      {syncDisabled && canEnableSync
        ? <Button intent="primary" disabled={busy || !loaded} onClick={enableSyncForVault}>Enable sync for this Vault</Button>
        : !syncDisabled && <Button intent="primary" disabled={busy || !loaded || drafts.length === 0} onClick={enable}>Enable sync</Button>}
    </>} />
  </Dialog>;
}
