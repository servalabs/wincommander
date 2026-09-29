export function FlowSettingsRecovery({ recoveryRequired }: { recoveryRequired: boolean }) {
  return (
    <section role="status" aria-live="polite" className="p-6 text-[var(--color-text-primary)]">
      <h2 className="text-lg font-semibold">
        {recoveryRequired ? "Automation needs personal data recovery" : "Automation is temporarily unavailable"}
      </h2>
      <p className="mt-2 text-sm">
        {recoveryRequired
          ? "Windows cannot unlock the personal data needed for your saved flows. Your original files are preserved. Automation stays locked until access is restored."
          : "WinCommander cannot access your saved personal settings right now. Automation stays paused until access is restored."}
        {" "}You can continue using the rest of WinCommander.
      </p>
    </section>
  );
}
