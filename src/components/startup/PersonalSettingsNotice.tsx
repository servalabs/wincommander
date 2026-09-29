import { useState } from "react";
import type { PersonalSettingsStatus } from "../../lib/startupHydration";
import { Button } from "../ui/button";

interface NoticeProps {
  status: PersonalSettingsStatus | null;
  onOpenSettings?: () => void;
}

export function PersonalSettingsNotice({ status, onOpenSettings }: NoticeProps) {
  if (!status || (!status.recoveryRequired && status.canSave)) return null;

  // A changed problem or a new app session must be acknowledged again.
  return <DismissibleNotice key={`${status.recoveryRequired}:${status.canSave}`} status={status} onOpenSettings={onOpenSettings} />;
}

function DismissibleNotice({ status, onOpenSettings }: NoticeProps & { status: PersonalSettingsStatus }) {
  const [dismissed, setDismissed] = useState(false);
  if (dismissed) return null;

  return (
    <div
      role="status"
      aria-live="polite"
      aria-atomic="true"
      className="shrink-0 border-b border-[var(--border)] bg-[var(--surface-2)] px-4 py-3 text-sm text-[var(--text)]"
    >
      <RecoveryMessage status={status} />
      <div className="mt-2 flex flex-wrap items-center gap-2">
        {onOpenSettings && <Button size="sm" onClick={onOpenSettings}>Review in Settings</Button>}
        <Button size="sm" variant="ghost" onClick={() => setDismissed(true)} aria-label="Dismiss personal data recovery notice for this session">Dismiss for now</Button>
        <span className="text-xs text-[var(--text-dim)]">Details remain in Settings. Dismissing does not unlock protected data.</span>
      </div>
    </div>
  );
}

function RecoveryMessage({ status }: { status: PersonalSettingsStatus }) {
  return (
    <>
      <p className="font-semibold">
        {status.recoveryRequired ? "Some saved personal data needs recovery" : "Personal settings are temporarily unavailable"}
      </p>
      <p>
        {status.recoveryRequired
          ? "Windows could not unlock some saved personal data. The original files are preserved. Unavailable preferences use safe defaults, and affected sensitive features stay locked until access is restored."
          : "WinCommander has opened with temporary personal defaults."}
        {status.canSave
          ? " You can save new preferences."
          : " Changes to personal preferences cannot be saved right now. Try reopening WinCommander when its background service is available."}
      </p>
    </>
  );
}

export function PersonalSettingsRecoveryDetails({ status }: { status: PersonalSettingsStatus | null }) {
  if (!status || (!status.recoveryRequired && status.canSave)) return null;
  return (
    <section aria-label="Personal data recovery" className="mb-4 rounded-lg border border-[var(--border)] bg-[var(--surface-2)] p-4 text-sm text-[var(--text)]">
      <RecoveryMessage status={status} />
      {status.recoveryRequired && <p className="mt-2">Use the Windows account that saved this data. Administrator permission alone cannot unlock another account's encrypted personal data. Keep the original files while restoring that account's access.</p>}
      <p className="mt-2 text-[var(--text-dim)]">After restoring access, reopen WinCommander to check again. Saving new preferences or dismissing the banner does not recover the original data.</p>
    </section>
  );
}
