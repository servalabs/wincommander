// SPDX-License-Identifier: AGPL-3.0-or-later
import { useCallback, useEffect, useId, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-shell";
import { Button, Dialog, DialogBody, DialogFooter } from "@/components/ui/bp";
import { Button as UiButton } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import "./PersonalVaultSyncDialog.css";
import useBackend from "@/hooks/useBackend";
import type { VaultSyncFolder } from "@/types/generated/ipc";
import { isSyncthingSetupUrl } from "@/lib/vaultSyncRecovery";
import { notifyVaultSyncRecovery } from "@/lib/vaultSyncWarning";
import { personalVaultSyncError, personalVaultSyncSetupMessage } from "@/lib/personalVaultSyncFeedback";
import { DEFAULT_VAULT_SYNC_LABEL, validVaultSyncLabel, validateVaultSyncDrafts, vaultSyncLabel, acceptsVaultSyncMountReceipt, type VaultSyncDraft } from "@/lib/personalVaultSyncManagement";
import { showSuccess } from "@/utils/toast";
import VaultOperationNotice from "./VaultOperationNotice";

interface Props { isOpen: boolean; onClose: () => void; internalDrive: number; driveLabel: string }
export default function PersonalVaultSyncDialog({ isOpen, onClose, internalDrive, driveLabel }: Props) {
  const fieldId = useId();
  const backend = useBackend();
  const latest = useRef(backend); latest.current = backend;
  const generation = useRef(0);
  const inFlight = useRef(false);
  const mountReceipt = useRef("");
  const returnFocus = useRef<HTMLElement | null>(null);
  const [folders, setFolders] = useState<VaultSyncFolder[]>([]);
  const [drafts, setDrafts] = useState<VaultSyncDraft[]>([{ path: "Sync", label: DEFAULT_VAULT_SYNC_LABEL }]);
  const [labels, setLabels] = useState<Record<string, string>>({});
  const [removeId, setRemoveId] = useState("");
  const [busy, setBusy] = useState(false);
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
    mountReceipt.current = result.mount_receipt;
    setFolders(result.folders);
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
      setDrafts([{ path: "Sync", label: DEFAULT_VAULT_SYNC_LABEL }]);
      setShowInfo(false);
      void refresh(epoch).catch(failure => {
        if (epoch !== generation.current) return;
        const detail = failure instanceof Error ? failure.message.toLowerCase() : String(failure).toLowerCase();
        setSyncDisabled(detail.includes("vault_syncthing_not_enabled"));
        setCanEnableSync(false);
        setError(personalVaultSyncError(failure));
      })
        .finally(() => { if (epoch === generation.current) { inFlight.current = false; setBusy(false); } });
    }
    return invalidate;
  }, [isOpen, internalDrive, refresh, invalidate]);
  const perform = async (operation: () => Promise<void>) => {
    if (inFlight.current) return;
    const epoch = generation.current;
    inFlight.current = true;
    setBusy(true); setError("");
    try { await operation(); }
    catch (failure) { if (epoch === generation.current) setError(personalVaultSyncError(failure)); }
    finally { if (epoch === generation.current) { inFlight.current = false; setBusy(false); } }
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
        } catch { throw new Error("vault_sync_label_update_failed"); }
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
  const close = () => { if (!inFlight.current) onClose(); };
  return <Dialog isOpen={isOpen} onClose={close} title="Syncthing setup" className="personal-vault-sync"
    onOpenAutoFocus={() => { returnFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null; }}
    onCloseAutoFocus={event => { if (returnFocus.current?.isConnected) { event.preventDefault(); returnFocus.current.focus({ preventScroll: true }); } }}
    style={{ width: "min(48rem, calc(100vw - 2rem))" }} canEscapeKeyClose={!busy} canOutsideClickClose={!busy} isCloseButtonShown={!busy}>
    <DialogBody>
      <p className="personal-vault-sync__intro">Sync selected folders with your phone or another device. Keep this Vault mounted during setup.</p>
      <div className="personal-vault-sync__context">
        <span className="personal-vault-sync__eyebrow">Selected Vault</span>
        <strong>{driveLabel}</strong>
        <span>Its synced folders pause on dismount and resume on mount.</span>
      </div>
      <UiButton variant="ghost" aria-expanded={showInfo} aria-controls={`${fieldId}-help`}
        onClick={() => setShowInfo(current => !current)}>{showInfo ? "Hide help" : "How setup works"}</UiButton>
      {showInfo && <div id={`${fieldId}-help`} className="personal-vault-sync__help">
        <p>Other Syncthing folders keep running when this Vault is dismounted.</p>
        <p>After setup, open Syncthing to share each configured folder with your device.</p>
      </div>}
      {syncDisabled && <p role="status" className="personal-vault-sync__help">{canEnableSync ? "Sync is off for this Vault. Enable it before adding folders." : "Sync is off for this Vault. Open Vault access, edit this Vault, turn on Syncthing, and save."}</p>}
      <section aria-labelledby={`${fieldId}-existing`} className="personal-vault-sync__section">
        <h3 id={`${fieldId}-existing`}>Existing synced folders</h3>
        {loaded && folders.length === 0 && <p className="personal-vault-sync__muted" role="status">No folders added yet. Add your first folder below.</p>}
        {folders.map((folder, index) => <div key={folder.folder_id} className="personal-vault-sync__folder">
          <div className="personal-vault-sync__folder-heading">
            <strong>{driveLabel}\{folder.relative_path}</strong>
            <span className="personal-vault-sync__status">{folder.recovery_required ? "Needs recovery" : folder.paused ? "Paused" : "Enabled"}</span>
          </div>
          <label htmlFor={`${fieldId}-saved-${index}`}>Syncthing display name</label>
          <Input id={`${fieldId}-saved-${index}`} value={labels[folder.folder_id] ?? folder.label} maxLength={128} disabled={busy}
            aria-describedby={`${fieldId}-name-help ${fieldId}-saved-error-${index}`} aria-invalid={!validVaultSyncLabel(vaultSyncLabel(labels[folder.folder_id]))}
            onChange={event => setLabels(current => ({ ...current, [folder.folder_id]: event.target.value }))} />
          {!validVaultSyncLabel(vaultSyncLabel(labels[folder.folder_id])) && <p id={`${fieldId}-saved-error-${index}`} role="alert">Use a display name of up to 128 bytes, without control characters. Shorten the name before saving.</p>}
          <div className="personal-vault-sync__actions">
            <Button disabled={busy || !validVaultSyncLabel(vaultSyncLabel(labels[folder.folder_id]))} onClick={() => void perform(async () => {
              await latest.current.managePersonalVaultSync(internalDrive, "rename", folder.relative_path, vaultSyncLabel(labels[folder.folder_id]), folder.folder_id, mountReceipt.current);
              await refresh(); showSuccess("Sync folder name updated. Its files and connected devices were kept.");
            })}>Save name</Button>
            {folder.recovery_required && <Button disabled={busy} onClick={() => recover(folder)}>Review recovery</Button>}
            <Button disabled={busy} onClick={() => setRemoveId(folder.folder_id)}>Remove sync</Button>
          </div>
          {removeId === folder.folder_id && <div role="alert" className="personal-vault-sync__confirmation">
            <p>Remove only this folder’s sync configuration? Files in the Vault and on your device stay in place. Other folders keep syncing.</p>
            <div className="personal-vault-sync__actions">
              <Button disabled={busy} intent="danger" onClick={() => void perform(async () => {
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
        <p id={`${fieldId}-name-help`} className="personal-vault-sync__muted">The Syncthing name is a label. It does not rename, move or delete files.</p>
        {drafts.map((draft, index) => <div key={index} className="personal-vault-sync__fields">
          <div>
            <label htmlFor={`${fieldId}-path-${index}`}>Vault-relative folder path</label>
            <Input id={`${fieldId}-path-${index}`} value={draft.path} maxLength={240} disabled={busy} placeholder="Phone\Camera"
              aria-describedby={`${fieldId}-path-help ${fieldId}-error`} aria-invalid={Boolean(error) && !pathsValid}
              onChange={event => setDrafts(current => current.map((item, i) => i === index ? { ...item, path: event.target.value } : item))} />
          </div>
          <div>
            <label htmlFor={`${fieldId}-label-${index}`}>Syncthing display name</label>
            <Input id={`${fieldId}-label-${index}`} value={draft.label} maxLength={128} disabled={busy}
              aria-describedby={`${fieldId}-name-help ${fieldId}-error`} aria-invalid={Boolean(error) && !validVaultSyncLabel(vaultSyncLabel(draft.label))}
              onChange={event => setDrafts(current => current.map((item, i) => i === index ? { ...item, label: event.target.value } : item))} />
          </div>
        </div>)}
        {drafts.length === 0 && <Button disabled={busy || folders.length >= 32} onClick={() => setDrafts([{ path: "Sync", label: DEFAULT_VAULT_SYNC_LABEL }])}>Add sync folder</Button>}
      </section>
      {busy && <p role="status">Checking sync… Keep this Vault mounted.</p>}
      <div id={`${fieldId}-error`}><VaultOperationNotice message={error} /></div>
    </DialogBody>
    <DialogFooter className="personal-vault-sync__footer" actions={<>
      <Button disabled={busy} onClick={() => void perform(() => refresh())}>Refresh</Button>
      {gui && <Button disabled={busy} onClick={() => void perform(() => open(gui))}>Open Syncthing</Button>}
      <Button disabled={busy} onClick={close}>Close</Button>
      {syncDisabled && canEnableSync
        ? <Button intent="primary" disabled={busy} onClick={enableSyncForVault}>Enable sync for this Vault</Button>
        : !syncDisabled && <Button intent="primary" disabled={busy || !loaded || drafts.length === 0} onClick={enable}>Enable sync</Button>}
    </>} />
  </Dialog>;
}
