// Post-change report for the firewall audit. Two things live here that the
// previous version computed and threw away:
//   · errors[] — only the COUNT was shown, so a user could never find out which
//     rule failed or why.
//   · backupPath — firewall_audit.rs exports the whole rule set before it
//     touches anything, and the path was never surfaced, leaving the most
//     reassuring part of a destructive feature invisible.
import { useState } from "react";
import useBackend from "../../hooks/useBackend";
import { Button } from "../../components/ui/button";
import { Icon } from "../../components/ui/icon";
import { backupFolderOf } from "../../lib/firewallAuditCopy";
import { MaintenanceNotice } from "./MaintenanceNotice";
import type { RemediationReport } from "./useFirewallAudit";

export function FirewallRemediationReport({ report }: { report: RemediationReport }) {
  const { openPath } = useBackend();
  const [copied, setCopied] = useState(false);
  const backupPath = report.backupPath;

  const copyPath = async () => {
    if (!backupPath) return;
    await navigator.clipboard.writeText(backupPath).catch(() => {});
    setCopied(true);
    setTimeout(() => setCopied(false), 1600);
  };

  return (
    <MaintenanceNotice tone={report.outcome.intent} headline={report.outcome.text}>
      {report.failures.length > 0 ? (
        <details className="mt-1">
          <summary className="cursor-pointer text-[12px] text-[var(--text)]">
            Why {report.failures.length} {report.failures.length === 1 ? "rule" : "rules"}{" "}
            {report.failures.length === 1 ? "was" : "were"} not changed
          </summary>
          <ul className="mt-1.5 flex flex-col gap-1">
            {report.failures.map((line) => (
              <li
                key={line}
                className="break-words font-[family-name:var(--font-mono)] text-[11px] text-[var(--text-dim)]"
              >
                {line}
              </li>
            ))}
          </ul>
        </details>
      ) : null}

      {backupPath ? (
        <div className="mt-2 rounded-[var(--r-sm)] border border-[var(--border)] bg-[var(--surface-2)] p-2">
          <div className="text-[11px] text-[var(--text-dim)]">
            Every firewall rule was exported before this change. Restore the whole set with{" "}
            <span className="font-[family-name:var(--font-mono)] text-[var(--text)]">
              netsh advfirewall import
            </span>
            .
          </div>
          <div className="mt-1 break-all font-[family-name:var(--font-mono)] text-[11px] text-[var(--text)]">
            {backupPath}
          </div>
          <div className="mt-1.5 flex flex-wrap gap-2">
            <Button variant="ghost" size="sm" onClick={() => void copyPath()}>
              <Icon icon={copied ? "tick" : "duplicate"} />
              {copied ? "Copied" : "Copy path"}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => void openPath(backupFolderOf(backupPath)).catch(() => {})}
            >
              <Icon icon="folder-open" />
              Open folder
            </Button>
          </div>
        </div>
      ) : null}
    </MaintenanceNotice>
  );
}
