// SPDX-License-Identifier: AGPL-3.0-or-later
import { useId, useRef, useState } from "react";
import { Button, Dialog, DialogBody, DialogFooter } from "@/components/ui/bp";
import { Input } from "@/components/ui/input";
import useVaultCloudBackup from "@/hooks/useVaultCloudBackup";
import { cloudBackupStateLabel, validCloudBackupPath } from "@/lib/vaultCloudBackup";
import VaultCloudBackupForm from "./VaultCloudBackupForm";
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
  const close = () => { if (!changing) { setConfirmation(null); setRestorePath(""); onClose(); } };
  const run = (operation: "disconnect" | "remove_setup") => {
    setConfirmation(null); void perform(operation);
  };
  return <Dialog isOpen={isOpen} onClose={close} title={`Cloud backup — ${driveLabel}`} className="vault-cloud-backup"
    style={{ width: "min(45rem, calc(100vw - 2rem))" }} canEscapeKeyClose={!changing} canOutsideClickClose={!changing} isCloseButtonShown={!changing}
    onOpenAutoFocus={() => { returnFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null; }}
    onCloseAutoFocus={event => { if (returnFocus.current?.isConnected) { event.preventDefault(); returnFocus.current.focus({ preventScroll: true }); } }}>
    <DialogBody className="vault-cloud-backup__body">
      <div className="vault-cloud-backup__disclosure">
        <strong>Copies files to your Proton Drive account</strong>
        <p>Remote copies are protected by your Proton account, independently of this Vault password. Deleting a file here does not delete its backup.</p>
        <p>The backup tool, saved login and settings stay inside this Vault. Windows, browser and network records may remain outside it.</p>
      </div>
      <p role="status" aria-live="polite">{busy ? action === "connect" ? "Complete the one-time Proton sign-in in your browser. Keep this Vault open." : action === "prepare" ? "Setting up the backup tool inside this Vault…" : action === "status" ? "Checking backup status…" : "Applying backup action… Keep this Vault open." : result ? cloudBackupStateLabel(result) : "Backup status is unavailable."}</p>
      <VaultOperationNotice message={error} />
      {result && <>
        <section className="vault-cloud-backup__section" aria-labelledby={`${id}-connection`}>
          <h3 id={`${id}-connection`}>Connection</h3>
          <dl className="vault-cloud-backup__summary"><dt>Backup tool</dt><dd>{result.engine_ready ? "Installed and verified" : "Not set up"}</dd>
            <dt>Proton account</dt><dd>{result.connected ? "Connected — saved session reused" : "Not connected"}</dd></dl>
          <div className="vault-cloud-backup__actions">
            {!result.engine_ready && <Button disabled={busy || !result.can_configure} onClick={() => void perform("prepare")}>Set up backup tool</Button>}
            {result.engine_ready && !result.connected && <Button disabled={busy || !result.can_configure} onClick={() => void perform("connect")}>Connect Proton Drive</Button>}
          </div>
        </section>
        {!result.can_configure && <p role="status">Cloud backup is not available to configure for this Windows account. Ask the Vault owner to enable it and grant write access.</p>}
        <VaultCloudBackupForm key={`${result.mount_receipt}-${result.configured}`} configuration={result.configuration}
          disabled={busy || !result.can_configure} canSave={result.engine_ready && result.connected && result.can_configure}
          onSave={configuration => void perform("configure", configuration)} />
        <section className="vault-cloud-backup__section" aria-labelledby={`${id}-last`}>
          <h3 id={`${id}-last`}>Last backup operation</h3>
          {result.last_run ? <dl className="vault-cloud-backup__summary">
            <dt>Outcome</dt><dd>{result.last_run.outcome === "complete" ? "Operation complete" : result.last_run.outcome}</dd>
            <dt>Transferred</dt><dd>{result.last_run.transferred_items} items · {result.last_run.transferred_bytes} bytes</dd>
            <dt>Unchanged</dt><dd>{result.last_run.skipped_items} items</dd><dt>Failed</dt><dd>{result.last_run.failed_items} items</dd>
            <dt>Started</dt><dd>{result.last_run.started_at}</dd><dt>Finished</dt><dd>{result.last_run.finished_at ?? "Not confirmed"}</dd>
          </dl> : <p>No completed backup result has been recorded.</p>}
          <p className="vault-cloud-backup__muted">An upload result is not proof of long-term retention or a successful restore. Verify checks matching content; test a restore too.</p>
          <div className="vault-cloud-backup__actions">
            <Button disabled={busy || !ready || result.running} onClick={() => void perform("start")}>Back up now</Button>
            <Button disabled={busy || (!result.running && !result.watching && result.reason_code !== "cloud_backup_sign_in_pending")} onClick={() => void perform("stop")}>Stop backup</Button>
            <Button disabled={busy || !ready || result.running} onClick={() => void perform("verify")}>Verify backup</Button>
          </div>
        </section>
        <section className="vault-cloud-backup__section" aria-labelledby={`${id}-restore`}>
          <h3 id={`${id}-restore`}>Restore to a new folder</h3>
          <label htmlFor={`${id}-restore-path`}>New folder inside this Vault</label>
          <Input id={`${id}-restore-path`} value={restorePath} maxLength={240} disabled={busy} placeholder="Restored files" onChange={event => setRestorePath(event.target.value)} />
          <p className="vault-cloud-backup__muted">Choose a new empty folder. Existing source files are not replaced.</p>
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
