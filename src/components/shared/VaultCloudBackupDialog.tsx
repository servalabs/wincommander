// SPDX-License-Identifier: AGPL-3.0-or-later
import { useId, useRef, useState } from "react";
import { Button, Dialog, DialogBody, DialogFooter } from "@/components/ui/bp";
import { Input } from "@/components/ui/input";
import useVaultCloudBackup from "@/hooks/useVaultCloudBackup";
import { cloudBackupRecordedRuns, cloudBackupRunSummary, cloudBackupStateLabel, validCloudBackupPath } from "@/lib/vaultCloudBackup";
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
  const [confirmation, setConfirmation] = useState<"disconnect" | "remove_setup" | null>(null);
  const changing = busy && action !== "status";
  const ready = Boolean(result?.engine_ready && result.connected && result.configured);
  const runs = result ? cloudBackupRecordedRuns(result) : null;
  const restoreSummary = runs?.restore ? cloudBackupRunSummary(runs.restore) : null;
  const close = () => { if (!changing) { setConfirmation(null); setRestorePath(""); onClose(); } };
  const run = (operation: "disconnect" | "remove_setup") => {
    setConfirmation(null); void perform(operation);
  };
  return <Dialog isOpen={isOpen} onClose={close} title={`Cloud backup — ${driveLabel}`} className="vault-cloud-backup"
    style={{ width: "min(45rem, calc(100vw - 2rem))" }} canEscapeKeyClose={!changing} canOutsideClickClose={!changing} isCloseButtonShown={!changing}
    onOpenAutoFocus={() => { returnFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null; }}
    onCloseAutoFocus={event => { if (returnFocus.current?.isConnected) { event.preventDefault(); returnFocus.current.focus({ preventScroll: true }); } }}>
    <DialogBody className="vault-cloud-backup__body">
      <p className="vault-cloud-backup__intro">Back up Vault files to your Proton Drive account.</p>
      <details className="vault-cloud-backup__details">
        <summary>About this backup</summary>
        <p>Proton protects cloud copies independently of your Vault password. Deleting local files keeps their cloud backups.</p>
        <p>The backup tool, saved login and settings stay inside this Vault. Windows, browser and network records may remain outside it.</p>
      </details>
      <p role="status" aria-live="polite">{busy ? action === "connect" ? "Complete the one-time Proton sign-in in your browser. Keep this Vault open." : action === "prepare" ? "Setting up the backup tool inside this Vault…" : action === "status" ? "Checking backup status…" : "Applying backup action… Keep this Vault open." : result ? cloudBackupStateLabel(result) : "Backup status is unavailable."}</p>
      <VaultOperationNotice message={error} />
      {result && <>
        <section className="vault-cloud-backup__section" aria-labelledby={`${id}-connection`}>
          <h3 id={`${id}-connection`}>Connection</h3>
          <dl className="vault-cloud-backup__summary"><dt>Backup tool</dt><dd>{result.engine_ready ? "Ready" : "Not set up"}</dd>
            <dt>Proton account</dt><dd>{result.connected ? "Connected" : "Not connected"}</dd></dl>
          <div className="vault-cloud-backup__actions">
            {!result.engine_ready && <Button disabled={busy || !result.can_configure} onClick={() => void perform("prepare")}>Set up backup tool</Button>}
            {result.engine_ready && !result.connected && <Button disabled={busy || !result.can_configure} onClick={() => void perform("connect")}>Connect Proton Drive</Button>}
          </div>
        </section>
        {!result.can_configure && <p role="status">Cloud backup is not available to configure for this Windows account. Ask the Vault owner to enable it and grant write access.</p>}
        <section className="vault-cloud-backup__section" aria-labelledby={`${id}-last`}>
          <h3 id={`${id}-last`}>Latest confirmations</h3>
          {runs?.backup && <VaultCloudBackupRunResult run={runs.backup} />}
          {runs?.verification && <VaultCloudBackupRunResult run={runs.verification} />}
          {runs?.previous && <VaultCloudBackupRunResult run={runs.previous} />}
          {!runs?.backup && !runs?.verification && !runs?.previous && <p className="vault-cloud-backup__muted">No backup result yet.</p>}
          {restoreSummary && <a className="vault-cloud-backup__muted" href={`#${id}-restore`}>Last restore: {restoreSummary.title}</a>}
          <div className="vault-cloud-backup__actions">
            <Button disabled={busy || !ready || result.running} onClick={() => void perform("start")}>Back up now</Button>
            <Button disabled={busy || (!result.running && !result.watching && result.reason_code !== "cloud_backup_sign_in_pending")} onClick={() => void perform("stop")}>Stop backup</Button>
            <Button disabled={busy || !ready || result.running} onClick={() => void perform("verify")}>Verify backup</Button>
          </div>
        </section>
        <VaultCloudBackupForm key={`${result.mount_receipt}-${result.configured}`} configuration={result.configuration}
          disabled={busy || !result.can_configure} canSave={result.engine_ready && result.connected && result.can_configure}
          onSave={configuration => void perform("configure", configuration)} />
        <section className="vault-cloud-backup__section" aria-labelledby={`${id}-restore`}>
          <h3 id={`${id}-restore`}>Restore to a new folder</h3>
          {runs?.restore && <VaultCloudBackupRunResult run={runs.restore} />}
          <label htmlFor={`${id}-restore-path`}>New folder inside this Vault</label>
          <Input id={`${id}-restore-path`} value={restorePath} maxLength={240} disabled={busy} placeholder="Restored files" onChange={event => setRestorePath(event.target.value)} />
          <p className="vault-cloud-backup__muted">Restores into a new folder; existing files stay intact.</p>
          <Button disabled={busy || !ready || result.running || !validCloudBackupPath(restorePath)} onClick={() => void perform("restore", undefined, restorePath)}>Restore files</Button>
        </section>
        <div className="vault-cloud-backup__actions">
          <Button disabled={busy || !result.connected} onClick={() => setConfirmation("disconnect")}>Disconnect Proton Drive</Button>
          <Button disabled={busy || (!result.configured && !result.engine_ready)} onClick={() => setConfirmation("remove_setup")}>Remove backup setup</Button>
        </div>
        {confirmation && <div role="alert" className="vault-cloud-backup__disclosure">
          <p>{confirmation === "disconnect" ? "Disconnect this saved Proton session? Future backups will need a new sign-in." : "Remove this Vault’s backup setup and saved session?"} Files already backed up in Proton Drive are kept.</p>
          <div className="vault-cloud-backup__actions"><Button intent="danger" disabled={busy} onClick={() => run(confirmation)}>Confirm {confirmation === "disconnect" ? "disconnect" : "remove setup"}</Button><Button disabled={busy} onClick={() => setConfirmation(null)}>Cancel</Button></div>
        </div>}
      </>}
    </DialogBody>
    <DialogFooter className="vault-cloud-backup__footer" actions={<><Button disabled={busy} onClick={() => void perform("status")}>Refresh</Button><Button disabled={changing} onClick={close}>Close</Button></>} />
  </Dialog>;
}
