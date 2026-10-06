// SPDX-License-Identifier: AGPL-3.0-or-later
import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-shell";
import { Button, Dialog, DialogBody, DialogFooter } from "@/components/ui/bp";
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
  const backend = useBackend();
  const latest = useRef(backend); latest.current = backend;
  const generation = useRef(0);
  const inFlight = useRef(false);
  const mountReceipt = useRef("");
  const [folders, setFolders] = useState<VaultSyncFolder[]>([]);
  const [drafts, setDrafts] = useState<VaultSyncDraft[]>([{ path: "Phone\\Camera", label: DEFAULT_VAULT_SYNC_LABEL }]);
  const [labels, setLabels] = useState<Record<string, string>>({});
  const [removeId, setRemoveId] = useState("");
  const [busy, setBusy] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState("");
  const [gui, setGui] = useState("");
  const refresh = async (epoch = generation.current) => {
    const result = await latest.current.managePersonalVaultSync(internalDrive, "list");
    if (epoch !== generation.current) return;
    if (!acceptsVaultSyncMountReceipt(mountReceipt.current, result.mount_receipt)) {
      setLoaded(false); setDrafts([]); setFolders([]); setGui("");
      throw new Error("vault_sync_mount_changed");
    }
    mountReceipt.current = result.mount_receipt;
    setFolders(result.folders);
    setLabels(Object.fromEntries(result.folders.map(folder => [folder.folder_id, folder.label])));
    setGui(isSyncthingSetupUrl(result.gui_url) ? result.gui_url : "");
    setLoaded(true);
  };
  useEffect(() => {
    const epoch = ++generation.current;
    if (isOpen) {
      mountReceipt.current = "";
      setLoaded(false); setError(""); setFolders([]); setGui(""); setRemoveId(""); setBusy(true);
      inFlight.current = true;
      setDrafts([{ path: "Phone\\Camera", label: DEFAULT_VAULT_SYNC_LABEL }]);
      void refresh(epoch).catch(failure => { if (epoch === generation.current) setError(personalVaultSyncError(failure)); })
        .finally(() => { if (epoch === generation.current) { inFlight.current = false; setBusy(false); } });
    }
    return () => { generation.current++; };
  }, [isOpen, internalDrive]);
  const perform = async (operation: () => Promise<void>) => {
    if (inFlight.current) return;
    inFlight.current = true;
    setBusy(true); setError("");
    try { await operation(); }
    catch (failure) { setError(personalVaultSyncError(failure)); }
    finally { inFlight.current = false; setBusy(false); }
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
  const recover = (folder: VaultSyncFolder) => void perform(async () => {
    const result = await latest.current.enablePersonalVaultSync(internalDrive, folder.relative_path, "inspect", undefined, mountReceipt.current);
    if (result.recovery_required) {
      onClose();
      window.setTimeout(() => notifyVaultSyncRecovery(driveLabel, internalDrive, result), 350);
    } else { await refresh(); }
  });
  return <Dialog isOpen={isOpen} onClose={() => { if (!busy) onClose(); }} title={`Sync folders in ${driveLabel}`}
    style={{ width: 620 }} canEscapeKeyClose={!busy} canOutsideClickClose={!busy} isCloseButtonShown={!busy}>
    <DialogBody>
      <p>Sync is your choice for this Vault. Mounting other personal or Fleet-created Vaults does not enable sync for them.</p>
      <p>These folders pause when this Vault is dismounted and resume when it is mounted. Other Syncthing folders keep running.</p>
      {loaded && folders.length === 0 && <p role="status">Sync is not enabled for this Vault. Add a folder below to enable it.</p>}
      {folders.map(folder => <div key={folder.folder_id} className="my-3 rounded-md border p-3 space-y-2">
        <p className="break-words">{driveLabel}\{folder.relative_path} — {folder.recovery_required ? "Needs recovery" : folder.paused ? "Paused" : "Enabled"}</p>
        <label className="block">Name shown in Syncthing
          <input value={labels[folder.folder_id] ?? folder.label} maxLength={128} disabled={busy} className="w-full rounded-md border p-2"
            onChange={event => setLabels(current => ({ ...current, [folder.folder_id]: event.target.value }))} />
        </label>
        <Button disabled={busy || !validVaultSyncLabel(vaultSyncLabel(labels[folder.folder_id]))} onClick={() => void perform(async () => {
          await latest.current.managePersonalVaultSync(internalDrive, "rename", folder.relative_path, vaultSyncLabel(labels[folder.folder_id]), folder.folder_id, mountReceipt.current);
          await refresh(); showSuccess("Sync folder name updated. Its files and connected devices were kept.");
        })}>Save name</Button>
        {folder.recovery_required && <Button disabled={busy} onClick={() => recover(folder)}>Review recovery</Button>}
        <Button disabled={busy} onClick={() => setRemoveId(folder.folder_id)}>Remove sync</Button>
        {removeId === folder.folder_id && <div role="alert">
          <p>Remove only this folder’s sync configuration? Files in the Vault and on your phone stay in place. Other folders keep syncing.</p>
          <Button disabled={busy} intent="danger" onClick={() => void perform(async () => {
            await latest.current.managePersonalVaultSync(internalDrive, "remove", folder.relative_path, undefined, folder.folder_id, mountReceipt.current);
            setRemoveId(""); await refresh(); showSuccess("Sync removed for this folder. Its files were kept.");
          })}>Confirm remove sync</Button>
          <Button disabled={busy} onClick={() => setRemoveId("")}>Keep syncing</Button>
        </div>}
      </div>)}
      <p>Choose separate folders, for example Phone\Camera and Phone\Documents. A name in Syncthing is a label; it does not rename or move files.</p>
      {drafts.map((draft, index) => <div key={index} className="my-3 rounded-md border p-3 space-y-2">
        <label className="block">Folder inside the Vault
          <input value={draft.path} maxLength={240} disabled={busy} className="w-full rounded-md border p-2" placeholder="Phone\Camera"
            onChange={event => setDrafts(current => current.map((item, i) => i === index ? { ...item, path: event.target.value } : item))} />
        </label>
        <label className="block">Name shown in Syncthing
          <input value={draft.label} maxLength={128} disabled={busy} className="w-full rounded-md border p-2"
            onChange={event => setDrafts(current => current.map((item, i) => i === index ? { ...item, label: event.target.value } : item))} />
        </label>
        <Button minimal disabled={busy} onClick={() => setDrafts(current => current.filter((_, i) => i !== index))}>Remove from setup</Button>
      </div>)}
      <Button disabled={busy || folders.length + drafts.length >= 32} onClick={() => setDrafts(current => [...current, { path: "", label: DEFAULT_VAULT_SYNC_LABEL }])}>Add another folder</Button>
      <p>After enabling, open Syncthing, connect your phone, and share each folder. To change a folder’s location, remove its sync configuration and add the new location; files are never moved automatically.</p>
      {busy && <p role="status">Checking sync… Keep this Vault mounted.</p>}
      <VaultOperationNotice message={error} />
    </DialogBody>
    <DialogFooter actions={<>
      <Button disabled={busy} onClick={() => void perform(() => refresh())}>Refresh</Button>
      {gui && <Button disabled={busy} onClick={() => void perform(() => open(gui))}>Open Syncthing</Button>}
      <Button disabled={busy} onClick={onClose}>Close</Button>
      <Button intent="primary" disabled={busy || !loaded || drafts.length === 0} onClick={enable}>Enable selected folders</Button>
    </>} />
  </Dialog>;
}
