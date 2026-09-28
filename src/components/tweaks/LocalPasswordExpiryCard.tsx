import { useCallback, useEffect, useMemo, useState } from "react";
import { Button, Spinner, Switch, Tag } from "@/components/ui/bp";
import SectionCard from "../shared/SectionCard";
import { executeBackendCommand } from "../../hooks/useBackend";
import { showSuccess } from "../../utils/toast";
import {
  formatPasswordExpirySummary,
  formatSkippedPasswordExpiryAccounts,
  type LocalPasswordExpiryStatus,
} from "./localPasswordExpiryUtils";

interface PasswordExpiryMutation {
  status: LocalPasswordExpiryStatus;
  changedCount: number;
  failedCount: number;
}

export default function LocalPasswordExpiryCard() {
  const [status, setStatus] = useState<LocalPasswordExpiryStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    const result = await executeBackendCommand<LocalPasswordExpiryStatus>("Get-LocalPasswordExpiryStatus");
    if (result.success && result.data) setStatus(result.data);
    else setError(result.error || "Windows could not read local password-expiry settings.");
    setLoading(false);
  }, []);

  useEffect(() => { void refresh(); }, [refresh]);

  const skipped = useMemo(() => status ? formatSkippedPasswordExpiryAccounts(status.skipped) : null, [status]);

  const setNeverExpires = async (enabled: boolean) => {
    if (!status) return;
    const message = enabled
      ? `Set Password never expires for all ${status.totalEligible} eligible local accounts? Domain, disabled, built-in, and service accounts stay unchanged.`
      : `Make all ${status.totalEligible} eligible local accounts follow the Windows password-age policy again? This does not restore each account's previous setting.`;
    if (!window.confirm(message)) return;

    setBusy(true);
    setError(null);
    const result = await executeBackendCommand<PasswordExpiryMutation>("Set-LocalPasswordNeverExpires", { Enabled: enabled });
    setBusy(false);
    if (result.success && result.data) {
      setStatus(result.data.status);
      const outcome = result.data.failedCount
        ? `${result.data.changedCount} account${result.data.changedCount === 1 ? "" : "s"} changed; ${result.data.failedCount} could not be changed.`
        : `${result.data.changedCount} account${result.data.changedCount === 1 ? "" : "s"} changed and Windows was rechecked.`;
      showSuccess(outcome);
    } else {
      setError(result.error || "Windows could not update local password-expiry settings.");
      void refresh();
    }
  };

  return (
    <SectionCard title="Local Account Password Expiry" icon="key" headerRight={status ? <Tag minimal intent={status.allEligibleNeverExpire ? "success" : "warning"}>{status.allEligibleNeverExpire ? "ALL NEVER EXPIRE" : "MIXED"}</Tag> : undefined}>
      {loading && !status ? (
        <div className="flex items-center gap-2 py-2 text-xs text-[var(--text-mute)]"><Spinner size={16} /> Reading local account settings…</div>
      ) : status ? (
        <div className="flex flex-col gap-3">
          <div className="flex items-start justify-between gap-4">
            <div>
              <div className="text-sm font-medium">Password never expires for eligible local accounts</div>
              <div className="mt-1 text-xs text-[var(--text-mute)]">
                {formatPasswordExpirySummary(status)}
              </div>
            </div>
            <Switch checked={status.allEligibleNeverExpire} disabled={!status.isAdmin || busy || status.totalEligible === 0} onChange={(event) => void setNeverExpires(event.currentTarget.checked)} aria-label="Set password never expires for eligible local accounts" />
          </div>

          {skipped && <div className="text-xs text-[var(--text-mute)]">{skipped}</div>}
          {status.totalEligible === 0 && <div className="text-xs text-[var(--text-mute)]">No eligible local password accounts were found. Domain accounts are never changed here.</div>}
          {!status.isAdmin && <div className="text-xs text-[var(--color-warning)]">Run WinCommander as administrator to change local account settings.</div>}
          {error && <div className="text-xs text-[var(--color-danger)]">{error}</div>}

          <div className="flex flex-wrap gap-2">
            <Button small minimal icon="refresh" text="Refresh" disabled={busy} onClick={() => void refresh()} />
          </div>
          <div className="text-[11px] leading-4 text-[var(--text-mute)]">This changes each eligible local account's Password never expires flag. It does not change Active Directory accounts or the computer-wide password-age policy.</div>
        </div>
      ) : <div className="text-xs text-[var(--color-danger)]">{error ?? "Local password-expiry status is unavailable."}</div>}
    </SectionCard>
  );
}
