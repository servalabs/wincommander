import type { PersonalSettingsStatus } from "../../lib/startupHydration";

export function PersonalSettingsNotice({ status }: { status: PersonalSettingsStatus | null }) {
  if (!status || (!status.recoveryRequired && status.canSave)) return null;

  return (
    <div
      role="status"
      aria-live="polite"
      aria-atomic="true"
      className="shrink-0 border-b border-[var(--color-border)] bg-[var(--color-bg-secondary)] px-4 py-3 text-sm text-[var(--color-text-primary)]"
    >
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
    </div>
  );
}
