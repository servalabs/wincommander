// SPDX-License-Identifier: AGPL-3.0-or-later
import { useId, useRef, useState } from "react";
import { Button, Dialog, DialogBody, DialogFooter } from "@/components/ui/bp";
import { Input } from "@/components/ui/input";
import { open } from "@tauri-apps/plugin-dialog";
import useVaultCloudBackup from "@/hooks/useVaultCloudBackup";
import { cloudBackupRecordedRuns, cloudBackupRunSummary, cloudBackupStateLabel, validCloudBackupPath } from "@/lib/vaultCloudBackup";
import { restorePathInPickedFolder } from "@/lib/vaultCloudBackupPicker";
import VaultCloudBackupForm from "./VaultCloudBackupForm";
import VaultCloudBackupRunResult from "./VaultCloudBackupRunResult";
import VaultOperationNotice from "./VaultOperationNotice";
import "./VaultCloudBackupDialog.css";

interface Props { isOpen: boolean; onClose: () => void; internalDrive: number; driveLabel: string; personal: boolean }
export default function VaultCloudBackupDialog({ isOpen, onClose, internalDrive, driveLabel, personal }: Props) {
  const id = useId();
  const returnFocus = useRef<HTMLElement | null>(null);
  const { result, busy, action, error, perform } = useVaultCloudBackup(isOpen, personal, internalDrive);
  const [restorePath, setRestorePath] = useState("");
  const [restorePickerError, setRestorePickerError] = useState("");
  const [pickingRestore, setPickingRestore] = useState(false);
  const [view, setView] = useState<"overview" | "settings" | "restore">("overview");
  const [confirmation, setConfirmation] = useState<"disconnect" | "remove_setup" | null>(null);
  // The backend's read-only check is distinct from the saved login and historical receipts.
  const checks = result as (typeof result & { connection_check?: "unknown" | "connected" | "signed_out"; remote_check?: "unknown" | "present" | "missing"; checked_at?: string | null; remote_path?: string | null });
  const ready = Boolean(result?.engine_ready && result.connected && result.configured && checks?.connection_check !== "signed_out");
  const runs = result ? cloudBackupRecordedRuns(result) : null;
  const restoreSummary = runs?.restore ? cloudBackupRunSummary(runs.restore) : null;
  const close = () => { setConfirmation(null); setRestorePath(""); setRestorePickerError(""); onClose(); };
  const chooseRestoreParent = async () => {
    setPickingRestore(true);
    try {
      const chosen = await open({ directory: true, multiple: false, defaultPath: `${driveLabel.replace(/[\\/]+$/, "")}\\`, title: "Choose a location inside this Vault" });
      if (!chosen) return;
      const next = restorePathInPickedFolder(driveLabel, chosen, restorePath);
      if (next === null) { setRestorePickerError("Choose a location inside this Vault."); return; }
      setRestorePath(next);
      setRestorePickerError("");
    } catch {
      setRestorePickerError("The folder picker could not be opened. Enter a new folder path inside this Vault.");
    } finally { setPickingRestore(false); }
  };
  const run = (operation: "disconnect" | "remove_setup") => {
    setConfirmation(null); void perform(operation);
  };
  return <Dialog isOpen={isOpen} onClose={close} title={`Cloud backup â€” ${driveLabel}`} className="vault-cloud-backup"
    style={{ width: "min(45rem, calc(100vw - 2rem))" }} canEscapeKeyClose canOutsideClickClose isCloseButtonShown
    onOpenAutoFocus={() => { returnFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null; }}
    onCloseAutoFocus={event => { if (returnFocus.current?.isConnected) { event.preventDefault(); returnFocus.current.focus({ preventScroll: true }); } }}>
    <DialogBody className="vault-cloud-backup__body">
      <div className="vault-cloud-backup__hero"><div><p className="vault-cloud-backup__eyebrow">PROTON DRIVE</p><p className="vault-cloud-backup__intro">Back up files from this Vault</p></div><div className="vault-cloud-backup__status" role="status" aria-live="polite">{(busy || result?.running) && <span className="vault-cloud-backup__spinner" aria-hidden="true" />}{busy ? action === "connect" ? "Complete the one-time Proton sign-in in your browser. Keep this Vault open." : action === "prepare" ? "Setting up the backup tool inside this Vaultâ€¦" : action === "status" ? "Checking backup statusâ€¦" : "Applying backup actionâ€¦ Keep this Vault open." : result ? cloudBackupStateLabel(result) : "Backup status is unavailable."}</div></div>
      <VaultOperationNotice message={error} />
      {result && <>
        <nav className="vault-cloud-backup__nav" aria-label="Cloud backup sections">
          {([ ["overview", "Overview"], ["settings", "Settings"], ["restore", "Restore"] ] as const).map(([key, label]) =>
            <button key={key} type="button" aria-pressed={view === key} onClick={() => setView(key)}>{label}</button>)}
        </nav>
        <div hidden={view !== "overview"} className="vault-cloud-backup__panel">
        <section className="vault-cloud-backup__section" aria-labelledby={`${id}-connection`}>
          <h3 id={`${id}-connection`}>Connection</h3>
          <dl className="vault-cloud-backup__summary"><dt>Backup tool</dt><dd>{result.engine_ready ? "Ready" : "Not set up"}</dd>
            <dt>Proton account</dt><dd>{checks?.connection_check === "connected" ? "Connected and checked" : checks?.connection_check === "signed_out" ? "Sign-in required" : result.connected ? "Saved login; connection not checked" : "Not connected"}</dd>
            {checks?.remote_path && <><dt>Destination</dt><dd>{["My files", ...checks.remote_path.replace(/^\/my-files\/?/i, "").split("/").filter(Boolean)].join(" / ")}</dd></>}
            {result.configured && <><dt>Backup folder</dt><dd>{checks?.remote_check === "present" ? "Found in Proton Drive" : checks?.remote_check === "missing" ? "Not found in Proton Drive" : "Not checked"}</dd></>}</dl>
          {checks?.remote_check === "present" && <p className="vault-cloud-backup__muted">Folder found; verify backup to check file contents.</p>}
          {checks?.remote_check === "missing" && <p role="status">The backup folder was not found. Back up again to create it.</p>}
          <div className="vault-cloud-backup__actions">
            {!result.engine_ready && <Button disabled={busy || !result.can_configure} onClick={() => void perform("prepare")}>Set up backup tool</Button>}
            {result.engine_ready && (!result.connected || checks?.connection_check === "signed_out") && <Button disabled={busy || !result.can_configure} onClick={() => void perform("connect")}>Connect Proton Drive</Button>}
            {result.engine_ready && result.connected && checks?.connection_check !== "signed_out" && !result.configured && <Button intent="primary" disabled={!result.can_configure} onClick={() => setView("settings")}>Configure backup</Button>}
          </div>
        </section>
        {!result.can_configure && <p role="status">Cloud backup is not available to configure for this Windows account. Ask the Vault owner to enable it and grant write access.</p>}
        <section className="vault-cloud-backup__section" aria-labelledby={`${id}-last`}>
          <h3 id={`${id}-last`}>Previous backup results</h3>
          <p className="vault-cloud-backup__muted">Results describe earlier checks; files may have changed since.</p>
          {runs?.backup && <VaultCloudBackupRunResult run={runs.backup} />}
          {runs?.verification && <VaultCloudBackupRunResult run={runs.verification} />}
          {runs?.previous && <VaultCloudBackupRunResult run={runs.previous} />}
          {!runs?.backup && !runs?.verification && !runs?.previous && <p className="vault-cloud-backup__muted">No backup result yet.</p>}
          {restoreSummary && <button type="button" className="vault-cloud-backup__link" onClick={() => setView("restore")}>Last restore: {restoreSummary.title}</button>}
          <div className="vault-cloud-backup__actions">
            <Button disabled={busy || !ready || result.running} onClick={() => void perform("start")}>Back up now</Button>
            <Button disabled={busy || (!result.running && !result.watching && result.reason_code !== "cloud_backup_sign_in_pending")} onClick={() => void perform("stop")}>Stop backup</Button>
            <Button disabled={busy || !ready || result.running} onClick={() => void perform("verify")}>Verify backup</Button>
          </div>
        </section>
        </div>
        <div hidden={view !== "settings"} className="vault-cloud-backup__panel">
        <VaultCloudBackupForm key={`${result.mount_receipt}-${result.configured}`} configuration={result.configuration}
          driveLabel={driveLabel}
          disabled={busy || !result.can_configure} canSave={result.engine_ready && result.connected && checks?.connection_check !== "signed_out" && result.can_configure}
          onSave={configuration => void perform("configure", configuration)} />
        <details className="vault-cloud-backup__details vault-cloud-backup__card"><summary>About this backup</summary>
          <p>Proton protects cloud copies independently of your Vault password. Deleting local files keeps their cloud backups.</p>
          <p>The backup tool, saved login and settings stay inside this Vault. Windows, browser and network records may remain outside it.</p>
        </details>
        <details className="vault-cloud-backup__details vault-cloud-backup__card"><summary>Manage connection</summary>
          <div className="vault-cloud-backup__actions">
            <Button disabled={busy || !result.connected} onClick={() => setConfirmation("disconnect")}>Disconnect Proton Drive</Button>
            <Button disabled={busy || (!result.configured && !result.engine_ready)} onClick={() => setConfirmation("remove_setup")}>Remove backup setup</Button>
          </div>
          {confirmation && <div role="alert" className="vault-cloud-backup__disclosure">
            <p>{confirmation === "disconnect" ? "Disconnect this saved Proton session? Future backups will need a new sign-in." : "Remove this Vault’s backup setup and saved session?"} Files already backed up in Proton Drive are kept.</p>
            <div className="vault-cloud-backup__actions"><Button intent="danger" disabled={busy} onClick={() => run(confirmation)}>Confirm {confirmation === "disconnect" ? "disconnect" : "remove setup"}</Button><Button disabled={busy} onClick={() => setConfirmation(null)}>Cancel</Button></div>
          </div>}
        </details>
        </div>
        <div hidden={view !== "restore"} className="vault-cloud-backup__panel">
        <section className="vault-cloud-backup__section" aria-labelledby={`${id}-restore`}>
          <h3 id={`${id}-restore`}>Restore to a new folder</h3>
          {runs?.restore && <VaultCloudBackupRunResult run={runs.restore} />}
          <label htmlFor={`${id}-restore-path`}>New folder inside this Vault</label>
          <Input id={`${id}-restore-path`} value={restorePath} maxLength={240} disabled={busy} placeholder="Restored files" onChange={event => setRestorePath(event.target.value)} />
          <Button disabled={busy || pickingRestore} onClick={() => void chooseRestoreParent()}>Choose restore location</Button>
          <p className="vault-cloud-backup__muted">Choose a parent folder inside this Vault; a new folder is created there. You can edit its name above.</p>
          {restorePickerError && <p role="alert">{restorePickerError}</p>}
          <Button disabled={busy || !ready || result.running || !validCloudBackupPath(restorePath)} onClick={() => void perform("restore", undefined, restorePath)}>Restore files</Button>
        </section>
        </div>
      </>}
    </DialogBody>
    <DialogFooter className="vault-cloud-backup__footer" actions={<><Button disabled={busy} onClick={() => void perform("status")}>Refresh</Button><Button onClick={close}>Close</Button></>} />
  </Dialog>;
}
