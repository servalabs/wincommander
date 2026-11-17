// SPDX-License-Identifier: AGPL-3.0-or-later
import type { VaultCloudBackupReceipt } from "@/types/generated/ipc";
import { cloudBackupRunSummary } from "@/lib/vaultCloudBackup";

export default function VaultCloudBackupRunResult({ run }: { run: VaultCloudBackupReceipt }) {
  const summary = cloudBackupRunSummary(run);
  return <>
    <div className="vault-cloud-backup__result" data-tone={summary.tone} role="status" aria-live="polite">
      <strong>{summary.title}</strong>
      {summary.detail && <p>{summary.detail}</p>}
      {(summary.destination || summary.finishedAt) && <dl className="vault-cloud-backup__summary">
        {summary.destination && <><dt>Destination</dt><dd>{summary.destination}</dd></>}
        {summary.finishedAt && <><dt>Finished</dt><dd><time dateTime={summary.finishedAt}>{new Date(summary.finishedAt).toLocaleString()}</time></dd></>}
      </dl>}
    </div>
    <details className="vault-cloud-backup__details">
      <summary>Transfer details</summary>
      <dl className="vault-cloud-backup__summary">
        <dt>Transferred</dt><dd>{run.transferred_items} items · {run.transferred_bytes} bytes</dd>
        <dt>Unchanged</dt><dd>{run.skipped_items} items</dd><dt>Failed</dt><dd>{run.failed_items} items</dd>
      </dl>
      <p className="vault-cloud-backup__muted">Item counts include folders.</p>
    </details>
  </>;
}
